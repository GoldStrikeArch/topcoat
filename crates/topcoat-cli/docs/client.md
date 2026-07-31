Topcoat can compile a client crate to JavaScript: plain Rust, compiled by the `rustc_codegen_js` backend, running in the browser. `topcoat client` builds the toolchain that compile needs, and `topcoat dev` builds it for you before every application build.

This is experimental. The backend lives in the `spike/rustc-codegen-js` checkout rather than on crates.io, so the commands here are behind the CLI's `client` feature:

```sh
cargo build -p topcoat-cli --bin topcoat --features client
```

# Why There Is Anything To Set Up

The backend is a rustc plugin. A plugin only loads into the exact nightly it was built against, so compiling a client crate needs that nightly, the backend built with it, a `core` compiled by the backend, and the view macros built for the host.

None of that has to touch your project. Your crate and the CLI stay on whatever toolchain you use, usually stable. Your build script asks `jsc-build` for a compile, and `jsc-build` runs the pinned nightly in child processes. The four things above are the one time setup those child processes need, and they take minutes to build.

Without the CLI, a missing piece surfaces as a build script error naming the command that builds it. With the CLI, the command gets run for you.

# Setting Up

From your project:

```sh
topcoat client setup
```

It reports what it had to build and does nothing when everything is in place:

```text
  client [demo-app] already ready
```

A checkout that needs work announces each step with the reason it is needed, then lets the step print as it runs, because the first setup takes minutes.

```text
  client [demo-app] backend: the codegen backend is not built at /spike/target/release/librustc_codegen_js.dylib
  client [demo-app] sysroot: there is no sysroot at /spike/build/sysroot
  client [demo-app] backend, sysroot
```

Pass `--package` to prepare one package of a workspace.

## Rebuilding

A step is performed when what it produces is missing. Whether an artifact is merely out of date cannot be told from the outside: the backend is a whole rustc plugin and the sysroot is `core` compiled by it, so nothing on disk says which sources they came from. Editing the backend means asking for the rebuild:

```sh
topcoat client setup --rebuild-backend   # the backend and the view macros
topcoat client setup --rebuild-sysroot   # the sysroot
```

# The Development Server

`topcoat dev` runs the same setup before each application build, so a cold checkout is prepared on the first build rather than reported as an error. A checkout that is ready prints nothing, and a project with no client crate costs one `cargo metadata` query.

The application build that follows is an ordinary `cargo build`. Your build script compiles the client crate from it, and `jsc-build` emits `cargo:rerun-if-changed` for the client sources, so editing a client source recompiles the client half and editing the server half does not.

# Configuration

There is nothing to configure in the common case. A package that has `jsc-build` as a build dependency compiles a client crate, and the checkout that dependency comes from is the checkout it is compiled with.

Two things cannot be inferred, and both live in `[package.metadata.topcoat.client]`:

```toml
[package.metadata.topcoat.client]
spike-root = "../rustc-codegen-js"
emit-args = ["js-minify=on"]
```

`spike-root` names the checkout, relative to the manifest, for a project whose `jsc-build` does not come from it. Declaring the table at all is enough to mark a package as compiling a client crate, so this is also how a package opts in without the build dependency.

`emit-args` repeats the emit-affecting backend options your build script passes to `BuildConfig::emit_arg`. Those options change how items are spelled, so the client crate and the sysroot's `core` have to agree on them, which means the setup has to know them to build a matching sysroot. Options passed to `BuildConfig::crate_arg` change one crate's own output and do not belong here.

The `JSC_SPIKE_ROOT` environment variable overrides the checkout for every package at once, the same way it overrides it for the build script.

# Sharing A Checkout

Several projects can compile against one checkout, and its setup is not safe to run twice at once: the steps build into one target directory and copy a patched standard library into one place. Runs are serialized with a lock file under the checkout's own target directory, so the first run proceeds while the rest wait and then find the work already done.

The setup also drops the `CARGO`, `RUSTC`, `RUSTFLAGS`, and `RUSTUP_TOOLCHAIN` variables from every step it runs. A `RUSTUP_TOOLCHAIN` inherited from your own build would otherwise override the nightly a step asks for by name, and the rest would shift the fingerprint of everything the step builds.

# Keeping The Pin Current

The nightly is pinned by `spike/rustc-codegen-js/rust-toolchain.toml`, and that pin is the one thing deciding which compiler the backend loads into. It cannot drift on its own, so a workflow walks it forward: once a week, `.github/workflows/jsc-nightly-bump.yml` reads the pin, resolves the latest nightly, installs it with the components the pin names, rebuilds the backend and the `core` it compiles, and runs every suite in the checkout.

It opens a pull request only if all of that passed. A bump pull request is therefore one line, the new channel in `rust-toolchain.toml`, plus the lockfile if it moved, and its evidence is the run that produced it: the backend built, the sysroot built, and the four suites green on the new nightly. There is nothing else in it to read.

A run that fails leaves the pin where it was. That is the useful outcome rather than a broken one: a nightly that drops a component the backend needs, stops loading the plugin, or moves a golden file is a nightly the pin should not be on yet. A golden that moved in particular is never bumped automatically, because deciding that new output is correct is a reading job.

The repository's own `rust-toolchain.toml` pins stable, and the bump never touches it. The nightly is installed beside stable and is only ever reached through the spike's toolchain file, so nothing about how your crate or the CLI builds changes.

Once a bump lands, an existing checkout still has a backend and a sysroot built by the previous nightly. Nothing on disk says so, which is why they have to be asked for:

```sh
topcoat client setup --rebuild-backend --rebuild-sysroot
```

A second workflow, `.github/workflows/jsc-contract-bump.yml`, runs monthly and checks something adjacent: that the pinned upstream sources the DOM contract is extracted from still produce the fixtures committed beside them. Drift there opens an issue with the regenerated tree attached, never a pull request, for the same reason a moved golden does not become a bump.
