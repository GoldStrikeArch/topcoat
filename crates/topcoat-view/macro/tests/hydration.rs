#![cfg(feature = "dom")]

use topcoat::{context::Cx, view::view};

/// Renders outside every island, which is how every page renders today.
fn plain(v: topcoat::Result, cx: &Cx) -> String {
    v.unwrap().render(cx)
}

/// Renders as the next island of the request.
fn island(v: topcoat::Result, cx: &Cx) -> String {
    let instance = cx.islands().next_instance();
    v.unwrap().island(instance).render(cx)
}

#[tokio::test]
async fn a_view_outside_an_island_carries_no_hydration_markup() {
    let cx = &Cx::default();
    let value = "x";
    assert_eq!(
        plain(view! { cx => <p class="a">"hello" (value)</p> }, cx),
        "<p class=\"a\">hellox</p>",
    );
}

#[tokio::test]
async fn a_template_root_is_keyed_inside_an_island() {
    let cx = &Cx::default();
    assert_eq!(
        island(view! { cx => <p>"hello"</p> }, cx),
        "<p data-hk=\"i0.0\">hello</p>",
    );
}

#[tokio::test]
async fn the_key_comes_before_the_other_attributes() {
    let cx = &Cx::default();
    assert_eq!(
        island(view! { cx => <p class="a" id="b"></p> }, cx),
        "<p data-hk=\"i0.0\" class=\"a\" id=\"b\"></p>",
    );
}

#[tokio::test]
async fn only_the_outermost_element_of_a_template_is_keyed() {
    let cx = &Cx::default();
    assert_eq!(
        island(view! { cx => <div><span><b>"a"</b></span></div> }, cx),
        "<div data-hk=\"i0.0\"><span><b>a</b></span></div>",
    );
}

#[tokio::test]
async fn sibling_templates_are_numbered_in_document_order() {
    let cx = &Cx::default();
    assert_eq!(
        island(view! { cx => <p></p><input><b></b> }, cx),
        "<p data-hk=\"i0.0\"></p><input data-hk=\"i0.1\"><b data-hk=\"i0.2\"></b>",
    );
}

#[tokio::test]
async fn keys_past_the_tenth_template_carry_the_ordinals_length() {
    let cx = &Cx::default();
    // The client counts its own keys as it claims nodes, and from ten it writes
    // the count's length into the key. A server that wrote `i0.10` here would be
    // describing nodes the client never asks for.
    assert_eq!(
        island(
            view! { cx =>
                <p></p> <p></p> <p></p> <p></p> <p></p> <p></p>
                <p></p> <p></p> <p></p> <p></p> <p></p> <p></p>
            },
            cx
        ),
        concat!(
            "<p data-hk=\"i0.0\"></p><p data-hk=\"i0.1\"></p><p data-hk=\"i0.2\"></p>",
            "<p data-hk=\"i0.3\"></p><p data-hk=\"i0.4\"></p><p data-hk=\"i0.5\"></p>",
            "<p data-hk=\"i0.6\"></p><p data-hk=\"i0.7\"></p><p data-hk=\"i0.8\"></p>",
            "<p data-hk=\"i0.9\"></p><p data-hk=\"i0.a10\"></p><p data-hk=\"i0.a11\"></p>",
        ),
    );
}

#[tokio::test]
async fn a_lone_dynamic_child_is_not_marked() {
    let cx = &Cx::default();
    let value = "x";
    assert_eq!(
        island(view! { cx => <p>(value)</p> }, cx),
        "<p data-hk=\"i0.0\">x</p>",
    );
}

#[tokio::test]
async fn a_dynamic_child_with_siblings_is_marked() {
    let cx = &Cx::default();
    let value = "x";
    assert_eq!(
        island(view! { cx => <p>"a" (value)</p> }, cx),
        "<p data-hk=\"i0.0\">a<!--$-->x<!--/--></p>",
    );
}

#[tokio::test]
async fn each_dynamic_child_gets_its_own_marker_pair() {
    let cx = &Cx::default();
    let (a, b) = ("x", "y");
    assert_eq!(
        island(view! { cx => <p>(a) (b)</p> }, cx),
        "<p data-hk=\"i0.0\"><!--$-->x<!--/--><!--$-->y<!--/--></p>",
    );
}

#[tokio::test]
async fn control_flow_is_marked_as_one_dynamic_child() {
    let cx = &Cx::default();
    let flag = true;
    assert_eq!(
        island(
            view! { cx => <div>"a" if flag { "b" } else { "c" }</div> },
            cx
        ),
        "<div data-hk=\"i0.0\">a<!--$-->b<!--/--></div>",
    );
}

#[tokio::test]
async fn a_loop_is_marked_once_around_all_of_its_rows() {
    let cx = &Cx::default();
    let items = ["a", "b"];
    // One marker pair around the whole run, and a key per row: the client builds
    // the row template once per item and asks for a key each time.
    assert_eq!(
        island(
            view! { cx => <ul>"x" for item in items { <li>(item)</li> }</ul> },
            cx
        ),
        concat!(
            "<ul data-hk=\"i0.0\">x<!--$-->",
            "<li data-hk=\"i0.1\">a</li><li data-hk=\"i0.2\">b</li>",
            "<!--/--></ul>",
        ),
    );
}

#[tokio::test]
async fn a_loop_with_no_rows_consumes_no_row_keys() {
    let cx = &Cx::default();
    let items: [&str; 0] = [];
    assert_eq!(
        island(
            view! { cx => <ul>"x" for item in items { <li>(item)</li> }</ul> "!" },
            cx
        ),
        "<ul data-hk=\"i0.0\">x<!--$--><!--/--></ul>!",
    );
}

#[tokio::test]
async fn a_branch_is_a_template_of_its_own() {
    let cx = &Cx::default();
    let flag = true;
    assert_eq!(
        island(
            view! { cx => <div>if flag { <span></span> } else { <b></b> }</div> },
            cx
        ),
        "<div data-hk=\"i0.0\"><span data-hk=\"i0.1\"></span></div>",
    );
}

#[tokio::test]
async fn only_the_branch_that_rendered_consumes_a_key() {
    let cx = &Cx::default();
    let flag = false;
    // The key is the second one written, not the second one the view declares:
    // the client never builds the branch it did not take, so it never counts
    // that branch's key off either.
    assert_eq!(
        island(
            view! { cx => <div>if flag { <span></span> } else { <b></b> }</div> },
            cx
        ),
        "<div data-hk=\"i0.0\"><b data-hk=\"i0.1\"></b></div>",
    );
}

#[tokio::test]
async fn keys_carry_on_past_a_branch() {
    let cx = &Cx::default();
    let flag = false;
    // What follows the branch is numbered after what the branch wrote, however
    // many keys that turned out to be.
    assert_eq!(
        island(
            view! { cx =>
                <div>if flag { <span><b></b></span> } else { <i></i> }</div>
                <hr>
            },
            cx
        ),
        "<div data-hk=\"i0.0\"><i data-hk=\"i0.1\"></i></div><hr data-hk=\"i0.2\">",
    );
}

#[tokio::test]
async fn static_children_are_never_marked() {
    let cx = &Cx::default();
    assert_eq!(
        island(view! { cx => <div>"a" <span></span> "b"</div> }, cx),
        "<div data-hk=\"i0.0\">a<span></span>b</div>",
    );
}

#[tokio::test]
async fn each_island_keys_against_its_own_instance() {
    let cx = &Cx::default();
    assert_eq!(
        island(view! { cx => <p>"a"</p> }, cx),
        "<p data-hk=\"i0.0\">a</p>",
    );
    assert_eq!(
        island(view! { cx => <p>"b"</p> }, cx),
        "<p data-hk=\"i1.0\">b</p>",
    );
}

#[tokio::test]
async fn an_island_adds_hydration_markup_and_nothing_else() {
    let cx = &Cx::default();
    let value = "x";
    let outside = plain(
        view! {
            cx =>
            <div>
                "a"
                (value)
            </div>
        },
        cx,
    );
    let inside = island(
        view! {
            cx =>
            <div>
                "a"
                (value)
            </div>
        },
        cx,
    );

    assert_eq!(outside, "<div>ax</div>");
    assert_eq!(
        inside
            .replace(" data-hk=\"i0.0\"", "")
            .replace("<!--$-->", "")
            .replace("<!--/-->", ""),
        outside,
    );
}
