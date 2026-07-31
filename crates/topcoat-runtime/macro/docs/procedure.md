A procedure is an async server function that the browser can call from inside a runtime [expression](macro.expr.html). Use procedures to run more complex Rust code that runtime expressions do not support, or to reach server-only resources like the database. Procedures are exposed as HTTP API endpoints from your server; parameters can be spoofed and **must not be trusted**.

```rust
use topcoat::{Result, runtime::procedure};

#[procedure]
async fn double(value: f64) -> Result<f64> {
    Ok(value * 2.0)
}
```

# Calling Procedures

Inside a runtime expression, call a procedure like an ordinary async function and `.await` its result:

```rust
# use topcoat::{Result, view::*, runtime::procedure};
# #[procedure]
# async fn double(value: f64) -> Result<f64> { Ok(value * 2.0) }
# #[component]
# async fn example() -> Result {
view! {
    signal count = 1.0;

    <button
        @click=$(async |_e| {
            let doubled = double(count.get()).await;
            count.set(doubled);
        })
    >
        "double it"
    </button>

    $(count.get())
}
# }
```

The call sends the arguments to the server, runs the function there, and resolves to its return value. Since that takes a network round-trip, calls are only possible in an async position, such as the body of an `async` closure.

A procedure call only runs in the browser. The server type-checks the call, but calling it there panics, so a call has to sit somewhere that never runs during the server render, like the closure bodies above.

# Arguments And Return Type

Argument types and the `Ok` type of the returned [`Result`] must belong to the shared vocabulary of [`expr!`], since their values cross between Rust and JavaScript.

A parameter named `cx` borrowing [`Cx`] is special: it is filled from the request context on the server and is not part of the call signature the client sees:

```rust
use topcoat::{Result, context::Cx, runtime::procedure};

#[procedure]
async fn search(cx: &Cx, query: String) -> Result<String> {
    // Query the database, read app context, check the session, ...
#   let _ = cx;
    Ok(query)
}
```

# Errors

Awaiting a call yields the procedure's `Ok` value directly. An `Err` becomes an error response, and the expression awaiting the call fails in the browser without a value; the error itself is not observable from the expression. If the client needs to react to failures, return the outcome as data instead, for example with an `Ok` type of `Result<String, String>`.

# Calling Procedures From Compiled Client Code

A procedure written as `#[procedure(serde)]` is called from client code that is compiled to the browser target, rather than from a runtime expression. Its arguments and its result cross as plain JSON, encoded by their own `Serialize` and `Deserialize` implementations, so they do not have to belong to the shared vocabulary of [`expr!`]:

```rust
use serde::{Deserialize, Serialize};
use topcoat::{Result, runtime::procedure};

#[derive(Serialize, Deserialize)]
struct Match {
    title: String,
    score: f64,
}

#[procedure(serde)]
async fn search(query: String) -> Result<Vec<Match>> {
    // Query the database, score the rows, ...
#   let _ = query;
    Ok(Vec::new())
}
```

The declaration compiles to one half per target. The server keeps the function body and the route. A crate compiled for the client instead gets a plain async function of the same name, taking the same arguments, which calls the procedure and resolves to its `Ok` value:

```text
let matches = search("topcoat".to_owned()).await?;
```

Because the body belongs to the server half alone, server-only code inside it never has to compile for the client.

A call takes a network round trip, so it only ever happens from somewhere that runs after the view is built: an event handler, or an effect. Building the view itself must not wait for one. That is a property of taking over server-rendered markup rather than a limitation of procedures: the client claims the nodes the server wrote in a single pass, and a pause in the middle of that pass ends it, leaving the client to build the whole subtree again from nothing.

A crate that declares a procedure this way says so in its manifest, since the two halves are selected by a cfg name:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(topcoat_client)'] }
```

## One Wire Per Procedure

A procedure serves one of the two wires, and which one is written on the declaration:

| declaration | called from | request `Content-Type` | arguments |
|---|---|---|---|
| `#[procedure]` | a runtime expression | `application/json` | the shared vocabulary's encoding |
| `#[procedure(serde)]` | compiled client code | `application/topcoat+json` | plain JSON, one value per argument |

A call that asks for the wrong wire is refused rather than decoded as if the two encodings were the same. The media type is what tells them apart, because the two bodies are both JSON: a call to a `#[procedure(serde)]` function that arrives without `Content-Type: application/topcoat+json` is answered with a bad-request error naming the media type it wanted.

Serving one wire is what frees the argument types: accepting both in one function would require every argument to belong to the shared vocabulary as well, which is the requirement the serde wire exists to lift. A procedure that both a runtime expression and compiled client code have to call is therefore declared twice, with the runtime-expression one calling the other's body.

The two wires also spell an empty argument list differently, and neither accepts the other's spelling: a call with no arguments sends `null` on the runtime-expression wire and `[]` on the serde wire.

# Registration

Each procedure is served by a route on the [`Router`]. `.discover()` registers every procedure linked into the binary; alternatively, mount procedures individually:

```rust
# use topcoat::{Result, router::Router, runtime::{procedure, RouterBuilderProcedureExt}};
# #[procedure]
# async fn double(value: f64) -> Result<f64> { Ok(value * 2.0) }
let router = Router::builder().procedure(double).build();
```

[`Cx`]: ../context/struct.Cx.html
[`Result`]: ../type.Result.html
[`Router`]: ../router/struct.Router.html
[`expr!`]: macro.expr.html
