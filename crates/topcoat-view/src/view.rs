use core::fmt;
use core::fmt::Write as _;
use std::borrow::Cow;

#[cfg(feature = "http")]
use http::{HeaderMap, StatusCode};
use smallvec::SmallVec;
use topcoat_core::context::Cx;
use topcoat_core::island::IslandInstance;

use crate::{Formatter, HtmlContext, HtmlWriter};

/// A self-contained piece of HTML content.
///
/// A view may contain multiple sibling nodes, but opened tags must be closed
/// so the fragment can be nested safely inside a larger document.
///
/// ```html
/// <!-- Valid: all tags are closed, safe to nest -->
/// <div>Hello</div>
/// <p>World</p>
///
/// <!-- Invalid: unclosed tag would corrupt the parent document -->
/// <div>Hello
/// ```
#[derive(Debug, Default, Clone)]
pub struct View {
    part: ViewPart,
}

impl View {
    /// Creates a view from accumulated view parts.
    ///
    /// This is called by generated `view!` code after collecting the nodes
    /// and attributes for a fragment.
    #[doc(hidden)]
    #[inline]
    #[must_use]
    pub fn new(parts: ViewParts) -> Self {
        Self { part: parts.into() }
    }

    /// Returns a `View` that renders to an empty string.
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a view from a `&'static str` without escaping it and without checking for syntax
    /// errors.
    #[inline]
    #[must_use]
    pub const fn unescaped_unchecked(body: &'static str) -> Self {
        Self {
            part: ViewPart::unescaped(body),
        }
    }

    /// Renders the view into an HTML string.
    #[cfg_attr(
        feature = "http",
        doc = "",
        doc = "Status codes and headers declared in the view are discarded;",
        doc = "[`render_response`](Self::render_response) collects them."
    )]
    pub fn render(&self, cx: &Cx) -> String {
        let mut buf = String::with_capacity(self.part.size_hint());
        let mut f = Formatter::new(&mut buf);
        self.part.render(cx, &mut f);
        buf
    }

    /// Renders the view into HTML together with the status code and response
    /// headers declared in it.
    ///
    /// A view declares response metadata by placing an
    /// [`http::StatusCode`](StatusCode), an [`http::HeaderMap`](HeaderMap),
    /// or a single `(HeaderName, HeaderValue)` pair in the node position of
    /// the `view!` macro. Competing declarations resolve by render order:
    /// the first status code rendered wins, and the first part that mentions
    /// a header name provides all of that name's values.
    #[cfg(feature = "http")]
    #[must_use]
    pub fn render_response(&self, cx: &Cx) -> RenderedResponse {
        let mut html = String::with_capacity(self.part.size_hint());
        let mut f = Formatter::new(&mut html);
        self.part.render(cx, &mut f);
        let (status_code, headers) = f.into_recorded();
        RenderedResponse {
            html,
            status_code,
            headers,
        }
    }

    /// Returns the view rendered as the island instance `instance`.
    ///
    /// Everything inside an island writes the hydration keys and markers that
    /// let the client find the nodes it has to take over, each key scoped to
    /// `instance`. Allocate the instance from the request with
    /// [`Cx::islands`](topcoat_core::context::Cx::islands) so that every island
    /// on a page gets its own key space.
    #[must_use]
    pub fn island(self, instance: IslandInstance) -> Self {
        Self {
            part: ViewPart::Island {
                size_hint: self.part.size_hint(),
                inner: Box::new(self.part),
                instance,
            },
        }
    }

    /// Returns the view rendered as a component's own view.
    ///
    /// A component's view numbers its hydration keys from zero, the way the
    /// island's own view does, so on its own a component rendered twice would
    /// write the same keys twice. Wrapping it here scopes them: the call takes
    /// the next ordinal the caller had reached and every key inside the
    /// component is written under it.
    ///
    /// Wrap the view of a component the client renders for itself, so the keys
    /// the server writes are the keys the client asks for. A component only the
    /// server renders needs no wrapper, and outside an island this changes
    /// nothing: no key is written there either way.
    #[must_use]
    pub fn component(self) -> Self {
        Self {
            part: ViewPart::Component {
                size_hint: self.part.size_hint(),
                inner: Box::new(self.part),
                child_content: None,
            },
        }
    }

    /// Returns the view rendered as a component's own view, called with `child`
    /// as its child content.
    ///
    /// Child content is an argument, so it is built where the component is
    /// called: it is rendered before the call takes its ordinal, numbering its
    /// keys in the caller's own numbering, and the bytes it wrote are spliced in
    /// wherever the component's view renders [`View::child_content`]. The keys a
    /// render then writes are no longer in document order, which is the point:
    /// the client builds child content as an argument too, so both sides number
    /// it before the component and neither numbers it inside.
    ///
    /// Child content the component's view never renders is numbered all the
    /// same, because building the argument is what spends the ordinals, on both
    /// sides.
    #[must_use]
    pub fn component_with_child(self, child: Self) -> Self {
        Self {
            part: ViewPart::Component {
                size_hint: self.part.size_hint() + child.part.size_hint(),
                inner: Box::new(self.part),
                child_content: Some(Box::new(child.part)),
            },
        }
    }

    /// Returns the view that renders the child content a component was called
    /// with.
    ///
    /// A component's view renders this where its child content belongs, and the
    /// call site hands the content over with
    /// [`component_with_child`](Self::component_with_child). Rendered anywhere
    /// else, it renders nothing.
    #[must_use]
    pub fn child_content() -> Self {
        Self {
            part: ViewPart::ChildContent,
        }
    }

    /// Returns the view rendered as `instance` and wrapped in the element a
    /// client finds an island by.
    ///
    /// The wrapper is the whole interface between a rendered island and the
    /// code that takes it over. It carries the island's `name`, the instance
    /// whose key prefix scopes the hydration keys inside it, and `seeds`, an
    /// already-serialized payload the client is started from. The wrapper
    /// itself is outside the island, so it carries no hydration key of its own
    /// and a client can use it as the element it hydrates into.
    ///
    /// The tag is a custom element name so that a runtime walking the document
    /// can recognize an island subtree and leave it to whoever owns it.
    #[must_use]
    pub fn island_element(
        self,
        name: impl Into<Cow<'static, str>>,
        instance: IslandInstance,
        seeds: impl Into<Cow<'static, str>>,
    ) -> Self {
        let mut parts = ViewParts::new();
        let mut w = PartsWriter::new(&mut parts, HtmlContext::AttributeValue);
        w.push_str_unescaped("<topcoat-island data-ti=\"");
        w.push_str(name);
        w.push_str_unescaped("\" data-tk=\"");
        w.push_str(instance.to_string());
        w.push_str_unescaped("\" data-ts=\"");
        w.push_str(seeds);
        w.push_str_unescaped("\">");
        parts.push_view(self.island(instance));
        PartsWriter::new(&mut parts, HtmlContext::AttributeValue)
            .push_str_unescaped("</topcoat-island>");
        Self::new(parts)
    }

    /// Unwraps the view into its root part.
    #[inline]
    pub(crate) fn into_part(self) -> ViewPart {
        self.part
    }
}

/// The custom element name an island's rendered subtree is wrapped in.
///
/// A client runtime that is not the one hydrating an island recognizes the
/// subtree by this tag and skips it, so the two never both drive the same
/// nodes.
pub const ISLAND_TAG: &str = "topcoat-island";

/// The output of rendering a [`View`] for an HTTP response.
///
/// Returned by [`View::render_response`]: the rendered HTML alongside the
/// status code and headers the view declared.
#[cfg(feature = "http")]
#[derive(Debug)]
#[non_exhaustive]
pub struct RenderedResponse {
    /// The rendered HTML.
    pub html: String,
    /// The first status code the render encountered, if any.
    pub status_code: Option<StatusCode>,
    /// The collected response headers.
    ///
    /// Each name carries the values of the first render part that mentioned
    /// it.
    pub headers: HeaderMap,
}

/// A renderable value stored in a [`View`].
///
/// View parts are created through a [`PartsWriter`] or the `view!` macro. A
/// part that holds text also records the [`HtmlContext`] it was written for,
/// so rendering escapes or validates it for exactly that position.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub enum ViewPart {
    /// Renders no content.
    #[default]
    Empty,
    /// A boolean rendered as text.
    #[non_exhaustive]
    Bool(bool),
    /// An `i8` rendered as text.
    #[non_exhaustive]
    I8(i8),
    /// An `i16` rendered as text.
    #[non_exhaustive]
    I16(i16),
    /// An `i32` rendered as text.
    #[non_exhaustive]
    I32(i32),
    /// An `i64` rendered as text.
    #[non_exhaustive]
    I64(i64),
    /// An `i128` rendered as text.
    #[non_exhaustive]
    I128(i128),
    /// An `isize` rendered as text.
    #[non_exhaustive]
    Isize(isize),
    /// A `u8` rendered as text.
    #[non_exhaustive]
    U8(u8),
    /// A `u16` rendered as text.
    #[non_exhaustive]
    U16(u16),
    /// A `u32` rendered as text.
    #[non_exhaustive]
    U32(u32),
    /// A `u64` rendered as text.
    #[non_exhaustive]
    U64(u64),
    /// A `u128` rendered as text.
    #[non_exhaustive]
    U128(u128),
    /// A `usize` rendered as text.
    #[non_exhaustive]
    Usize(usize),
    /// An `f32` rendered as text.
    #[non_exhaustive]
    F32(f32),
    /// An `f64` rendered as text.
    #[non_exhaustive]
    F64(f64),
    /// A character rendered for the recorded context.
    #[non_exhaustive]
    Char { value: char, context: HtmlContext },
    /// A string rendered for the recorded context.
    #[non_exhaustive]
    Str {
        value: Cow<'static, str>,
        context: HtmlContext,
    },
    /// A custom view part that writes its output at render time.
    #[non_exhaustive]
    BoxDyn {
        inner: Box<dyn DynViewPart>,
        context: HtmlContext,
        size_hint: usize,
    },
    /// A sequence of view parts rendered in order.
    #[non_exhaustive]
    BoxSlice {
        inner: Box<[ViewPart]>,
        size_hint: usize,
    },
    /// A hydration site, which renders only inside an island.
    #[non_exhaustive]
    HydrationKey { site: HydrationSite },
    /// A subtree rendered as one island instance.
    #[non_exhaustive]
    Island {
        instance: IslandInstance,
        inner: Box<ViewPart>,
        size_hint: usize,
    },
    /// A subtree rendered as a component's own view, whose hydration keys nest
    /// under the ordinal the call took, and the child content the call passed it.
    #[non_exhaustive]
    Component {
        inner: Box<ViewPart>,
        child_content: Option<Box<ViewPart>>,
        size_hint: usize,
    },
    /// The child content a component was called with, rendered where its view
    /// puts it.
    #[non_exhaustive]
    ChildContent,
    /// A response status code recorded at render time; renders no content.
    #[cfg(feature = "http")]
    #[non_exhaustive]
    StatusCode(StatusCode),
    /// Response headers recorded at render time; renders no content.
    #[cfg(feature = "http")]
    #[non_exhaustive]
    Headers(Box<HeaderMap>),
}

impl ViewPart {
    /// Returns an empty view part.
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Returns `true` if the view part is [`Empty`].
    ///
    /// [`Empty`]: ViewPart::Empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns a part that renders `value` verbatim.
    #[inline]
    pub(crate) const fn unescaped(value: &'static str) -> Self {
        Self::Str {
            value: Cow::Borrowed(value),
            context: HtmlContext::Unescaped,
        }
    }

    /// Writes the part into `f`, escaped or validated for the context each
    /// piece of text was written in.
    pub(crate) fn render(&self, cx: &Cx, f: &mut Formatter<'_>) {
        let mut int_buffer = itoa::Buffer::new();

        match self {
            Self::Empty => {}
            Self::Bool(inner) => f.write_str(if *inner { "true" } else { "false" }),
            // The `Display` output of the numeric types consists of digits,
            // signs, and plain letters, none of which are significant in any
            // HTML context, so they write verbatim.
            Self::I8(inner) => f.write_str(int_buffer.format(*inner)),
            Self::I16(inner) => f.write_str(int_buffer.format(*inner)),
            Self::I32(inner) => f.write_str(int_buffer.format(*inner)),
            Self::I64(inner) => f.write_str(int_buffer.format(*inner)),
            Self::I128(inner) => f.write_str(int_buffer.format(*inner)),
            Self::Isize(inner) => f.write_str(int_buffer.format(*inner)),
            Self::U8(inner) => f.write_str(int_buffer.format(*inner)),
            Self::U16(inner) => f.write_str(int_buffer.format(*inner)),
            Self::U32(inner) => f.write_str(int_buffer.format(*inner)),
            Self::U64(inner) => f.write_str(int_buffer.format(*inner)),
            Self::U128(inner) => f.write_str(int_buffer.format(*inner)),
            Self::Usize(inner) => f.write_str(int_buffer.format(*inner)),
            Self::F32(inner) => write!(f, "{inner}").unwrap(),
            Self::F64(inner) => write!(f, "{inner}").unwrap(),
            Self::Char { value, context } => context.writer(f).write_char(*value),
            Self::Str { value, context } => context.writer(f).write_str(value),
            Self::BoxDyn { inner, context, .. } => inner.render(cx, &mut context.writer(f)),
            Self::BoxSlice { inner, .. } => {
                for part in inner {
                    part.render(cx, f);
                }
            }
            Self::HydrationKey { site } => {
                if let Some(instance) = f.island() {
                    site.render(instance, f);
                }
            }
            Self::Island {
                instance, inner, ..
            } => {
                let previous = f.enter_island(*instance);
                inner.render(cx, f);
                f.restore(previous);
            }
            Self::Component {
                inner,
                child_content,
                ..
            } => {
                // Child content is an argument, so it is rendered here, where
                // the component is called and before the call takes its
                // ordinal. Only its bytes wait for the view to render them.
                let child_content = child_content
                    .as_ref()
                    .map(|content| f.write_aside(|f| content.render(cx, f)));
                let previous = f.enter_component(child_content);
                inner.render(cx, f);
                f.restore(previous);
            }
            Self::ChildContent => f.write_child_content(),
            #[cfg(feature = "http")]
            Self::StatusCode(status_code) => f.record_status_code(*status_code),
            #[cfg(feature = "http")]
            Self::Headers(headers) => f.record_headers(headers),
        }
    }

    /// Returns an estimate of the number of bytes this part will write.
    ///
    /// Used to pre-allocate the output buffer. A slight over-estimate is
    /// preferable to an under-estimate: falling short forces the buffer to
    /// grow and copy, whereas a modest over-estimate only leaves a little
    /// capacity unused.
    pub(crate) fn size_hint(&self) -> usize {
        // Each numeric hint is the midpoint, rounded up, between the shortest
        // and widest output the type can `Display`, including the leading `-`
        // for signed types (`isize`/`usize` assume a 64-bit target). A
        // float's `Display` width is unbounded for extreme magnitudes, so the
        // upper end is the shortest round-trip form of a typical value.
        #[allow(clippy::match_same_arms)]
        match self {
            Self::Empty => 0,
            Self::Bool(_) => 5,
            Self::I8(_) => 3,
            Self::I16(_) => 4,
            Self::I32(_) => 6,
            Self::I64(_) => 11,
            Self::I128(_) => 21,
            Self::Isize(_) => 11,
            Self::U8(_) => 2,
            Self::U16(_) => 3,
            Self::U32(_) => 6,
            Self::U64(_) => 11,
            Self::U128(_) => 20,
            Self::Usize(_) => 11,
            Self::F32(_) => 9,
            Self::F64(_) => 13,
            // One to four UTF-8 bytes, or an escape sequence.
            Self::Char { .. } => 3,
            Self::Str { value, context } => match context {
                HtmlContext::Unescaped => value.len(),
                // Assume some characters escape into multi-byte sequences.
                _ => value.len() + value.len() / 8,
            },
            Self::BoxDyn { size_hint, .. }
            | Self::BoxSlice { size_hint, .. }
            | Self::Island { size_hint, .. }
            | Self::Component { size_hint, .. } => *size_hint,
            Self::HydrationKey { site, .. } => site.size_hint(),
            // The content is sized where the call site passed it in.
            Self::ChildContent => 0,
            #[cfg(feature = "http")]
            Self::StatusCode(_) | Self::Headers(_) => 0,
        }
    }
}

/// A position in a view that hydration has to be able to find again.
///
/// A site renders only while the render is inside an island, so a view that is
/// never rendered as one produces markup with no trace of hydration in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HydrationSite {
    /// The outermost element of a template, which is the only element the
    /// client looks up by key: everything below it is reached by walking down
    /// from it. Renders as a `data-hk` attribute and so belongs inside the
    /// element's opening tag, after the tag name.
    TemplateRoot,
    /// The start of a child region whose content is replaced at runtime.
    ChildStart,
    /// The end of a child region whose content is replaced at runtime.
    ChildEnd,
}

impl HydrationSite {
    /// Writes this site inside `instance`.
    ///
    /// A template root takes the next ordinal the island has to hand, so keys
    /// are numbered in the order they are written, and writes it in the
    /// component nesting the render is inside.
    fn render(self, instance: IslandInstance, f: &mut Formatter<'_>) {
        match self {
            // A key is decimal digits and lowercase letters, so nothing here
            // needs escaping for the attribute value it is written into.
            Self::TemplateRoot => {
                f.write_str(" data-hk=\"");
                f.write_next_key(instance);
                f.write_str("\"");
            }
            // The long comment form, which is what a browser parsing a served
            // document expects. The short `<!$>` form is only valid inside a
            // template string handed to `innerHTML`. A marker is found by
            // walking out from the node before it rather than by key, so it
            // takes no ordinal on either side.
            Self::ChildStart => f.write_str("<!--$-->"),
            Self::ChildEnd => f.write_str("<!--/-->"),
        }
    }

    /// An estimate of the number of bytes this site writes inside an island.
    fn size_hint(self) -> usize {
        match self {
            // ` data-hk="i0.a10"`, so a second digit in each half plus the
            // letter a second digit brings with it.
            Self::TemplateRoot => 18,
            Self::ChildStart | Self::ChildEnd => 8,
        }
    }
}

/// A boxed view part that writes its output at render time.
///
/// Implement this for values whose output is only known when the view
/// renders, such as resolved asset URLs. The writer passed to
/// [`render`](Self::render) already carries the [`HtmlContext`] of the
/// position the part was pushed into, so everything written through it is
/// escaped or validated for that position.
pub trait DynViewPart: 'static + fmt::Debug + Send {
    /// Writes this part's output into `w`.
    fn render(&self, cx: &Cx, w: &mut HtmlWriter<'_, '_>);

    /// Returns an estimate of the number of bytes this part will write.
    ///
    /// Used to pre-allocate the output buffer, so aim for a close estimate. A
    /// slight over-estimate is usually preferable to an under-estimate.
    #[inline]
    fn size_hint(&self) -> usize {
        0
    }

    /// Clones this view part into a fresh boxed value.
    fn clone_box(&self) -> Box<dyn DynViewPart>;
}

impl Clone for Box<dyn DynViewPart> {
    #[inline]
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}

/// A buffer collecting renderable values before they become a [`View`].
///
/// This is plumbing for generated `view!` code, which fills the buffer
/// through [`PartsWriter`] lenses and the position helpers and finally passes
/// it to `View::new`.
#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct ViewParts {
    items: SmallVec<[ViewPart; 8]>,
}

impl ViewParts {
    /// Creates an empty view-parts buffer.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a nested view, such as a rendered component.
    #[inline]
    pub fn push_view(&mut self, view: View) -> &mut Self {
        self.items.push(view.into_part());
        self
    }

    /// Appends an already-sealed view part.
    ///
    /// A part records the [`HtmlContext`] its text was written for. Pushing
    /// it into a position with different escaping requirements bypasses that
    /// protection, so this is reserved for framework plumbing that re-emits
    /// parts in the position family they were built for.
    #[doc(hidden)]
    #[inline]
    pub fn push_part(&mut self, part: ViewPart) -> &mut Self {
        self.items.push(part);
        self
    }

    /// Appends a hydration site.
    ///
    /// The site renders nothing unless the view is rendered as an island, so
    /// generated code can push one wherever the key plan has a site without
    /// changing the markup of a view that is never hydrated. A site that carries
    /// a key is numbered when it renders, not here: only a render knows which
    /// branches it took.
    #[doc(hidden)]
    #[inline]
    pub fn push_hydration_site(&mut self, site: HydrationSite) -> &mut Self {
        self.items.push(ViewPart::HydrationKey { site });
        self
    }
}

impl From<ViewParts> for ViewPart {
    #[inline]
    fn from(mut value: ViewParts) -> Self {
        match value.items.len() {
            0 => ViewPart::Empty,
            1 => value.items.pop().unwrap(),
            _ => {
                let size_hint = value.items.iter().map(ViewPart::size_hint).sum();
                ViewPart::BoxSlice {
                    inner: value.items.into_boxed_slice(),
                    size_hint,
                }
            }
        }
    }
}

macro_rules! impl_push_primitive {
    ($method:ident, $ty:ty, $variant:ident) => {
        #[doc = concat!("Appends a `", stringify!($ty), "` rendered as text.")]
        ///
        /// Its rendered form contains no character that is significant in any
        /// HTML context, so no escaping applies.
        #[inline]
        pub fn $method(&mut self, value: $ty) -> &mut Self {
            self.parts.items.push(ViewPart::$variant(value));
            self
        }
    };
}

/// A context-carrying writer over a view-parts buffer, created per position.
///
/// The `view!` macro creates a `PartsWriter` for each dynamic position it
/// fills and hands it to the matching position trait:
/// [`NodeViewParts`](crate::NodeViewParts),
/// [`AttributeValueViewParts`](crate::AttributeValueViewParts),
/// [`AttributeKeyViewParts`](crate::AttributeKeyViewParts),
/// [`ElementNameViewParts`](crate::ElementNameViewParts), or
/// [`AttributeViewParts`](crate::AttributeViewParts).
///
/// Implementations of those traits make a value renderable by pushing it
/// through the `push_*` methods, which seal the pushed text with the
/// [`HtmlContext`] of the position so rendering escapes or validates it
/// correctly, or by delegating to another implementation of the same
/// position trait. [`push_str_unescaped`](Self::push_str_unescaped) is the
/// only way to opt out of that protection.
pub struct PartsWriter<'a> {
    parts: &'a mut ViewParts,
    context: HtmlContext,
}

impl<'a> PartsWriter<'a> {
    /// Creates a writer that seals everything pushed into it with `context`.
    #[inline]
    pub fn new(parts: &'a mut ViewParts, context: HtmlContext) -> Self {
        Self { parts, context }
    }

    /// Returns a writer over the same buffer for a different context.
    ///
    /// In-crate compositions that span more than one position use this to
    /// transition between the positions they cover, such as
    /// [`Attribute`](crate::Attribute) moving from a key to a value or
    /// [`push_comment`](Self::push_comment) sealing a comment body.
    #[inline]
    pub(crate) fn with_context(&mut self, context: HtmlContext) -> PartsWriter<'_> {
        PartsWriter {
            parts: self.parts,
            context,
        }
    }

    /// Appends a string, sealed with this writer's context.
    #[inline]
    pub fn push_str(&mut self, value: impl Into<Cow<'static, str>>) -> &mut Self {
        self.parts.items.push(ViewPart::Str {
            value: value.into(),
            context: self.context,
        });
        self
    }

    /// Appends a string that renders verbatim, bypassing this writer's
    /// context.
    ///
    /// Use this only for trusted markup. Passing untrusted input defeats the
    /// runtime's escaping and can lead to XSS vulnerabilities.
    #[inline]
    pub fn push_str_unescaped(&mut self, value: impl Into<Cow<'static, str>>) -> &mut Self {
        self.parts.items.push(ViewPart::Str {
            value: value.into(),
            context: HtmlContext::Unescaped,
        });
        self
    }

    /// Appends an HTML comment whose body is built through `build`.
    ///
    /// The `<!-- ` and ` -->` delimiters are written verbatim, while the
    /// writer handed to `build` seals everything pushed into it for the
    /// [`Comment`](HtmlContext::Comment) context. Because that context
    /// escapes `>`, the body can never contain `-->` and terminate the
    /// comment, so a marker can be built from untrusted data with
    /// [`push_str`](Self::push_str) and no separate escaping step.
    ///
    /// # Panics
    ///
    /// Panics if used in a non-text HTML context.
    #[inline]
    pub fn push_comment(&mut self, build: impl FnOnce(&mut PartsWriter<'_>)) -> &mut Self {
        assert!(
            self.context == HtmlContext::Text,
            "tried to push comment in html context {:?}",
            self.context,
        );
        self.push_str_unescaped("<!-- ");
        build(&mut self.with_context(HtmlContext::Comment));
        self.push_str_unescaped(" -->");
        self
    }

    /// Appends a character, sealed with this writer's context.
    #[inline]
    pub fn push_char(&mut self, value: char) -> &mut Self {
        self.parts.items.push(ViewPart::Char {
            value,
            context: self.context,
        });
        self
    }

    impl_push_primitive!(push_bool, bool, Bool);
    impl_push_primitive!(push_i8, i8, I8);
    impl_push_primitive!(push_i16, i16, I16);
    impl_push_primitive!(push_i32, i32, I32);
    impl_push_primitive!(push_i64, i64, I64);
    impl_push_primitive!(push_i128, i128, I128);
    impl_push_primitive!(push_isize, isize, Isize);
    impl_push_primitive!(push_u8, u8, U8);
    impl_push_primitive!(push_u16, u16, U16);
    impl_push_primitive!(push_u32, u32, U32);
    impl_push_primitive!(push_u64, u64, U64);
    impl_push_primitive!(push_u128, u128, U128);
    impl_push_primitive!(push_usize, usize, Usize);
    impl_push_primitive!(push_f32, f32, F32);
    impl_push_primitive!(push_f64, f64, F64);

    /// Appends a part that writes its output at render time, sealed with
    /// this writer's context.
    #[inline]
    pub fn push_dyn(&mut self, part: Box<dyn DynViewPart>) -> &mut Self {
        self.parts.items.push(ViewPart::BoxDyn {
            size_hint: part.size_hint(),
            inner: part,
            context: self.context,
        });
        self
    }

    /// Appends an already-sealed view part.
    ///
    /// A part records the [`HtmlContext`] its text was written for; this
    /// writer's context does not apply. See [`ViewParts::push_part`].
    #[doc(hidden)]
    #[inline]
    pub fn push_part(&mut self, part: ViewPart) -> &mut Self {
        self.parts.items.push(part);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(build: impl FnOnce(&mut ViewParts)) -> String {
        let mut parts = ViewParts::new();
        build(&mut parts);
        View::new(parts).render(&Cx::default())
    }

    #[test]
    fn empty_view_renders_empty() {
        assert_eq!(View::empty().render(&Cx::default()), "");
    }

    #[test]
    fn unescaped_unchecked_renders_verbatim() {
        let view = View::unescaped_unchecked("<b>raw</b>");
        assert_eq!(view.render(&Cx::default()), "<b>raw</b>");
    }

    #[test]
    fn push_str_seals_the_writer_context() {
        let out = render(|parts| {
            PartsWriter::new(parts, HtmlContext::Text).push_str("<b> & \"q\"");
        });
        assert_eq!(out, "&lt;b&gt; &amp; \"q\"");

        let out = render(|parts| {
            PartsWriter::new(parts, HtmlContext::AttributeValue).push_str("<b> & \"q\"");
        });
        assert_eq!(out, "<b> &amp; &quot;q&quot;");
    }

    #[test]
    fn push_str_unescaped_bypasses_the_context() {
        let out = render(|parts| {
            PartsWriter::new(parts, HtmlContext::Text).push_str_unescaped("<b>raw</b>");
        });
        assert_eq!(out, "<b>raw</b>");
    }

    #[test]
    fn push_char_seals_the_writer_context() {
        let out = render(|parts| {
            PartsWriter::new(parts, HtmlContext::Text).push_char('<');
        });
        assert_eq!(out, "&lt;");
    }

    #[test]
    #[should_panic(expected = "invalid attribute key")]
    fn ident_context_panics_on_forbidden_characters_at_render() {
        render(|parts| {
            PartsWriter::new(parts, HtmlContext::AttributeKey).push_str("on click");
        });
    }

    #[test]
    fn push_primitives_render_as_text() {
        let out = render(|parts| {
            let mut writer = PartsWriter::new(parts, HtmlContext::Text);
            writer.push_i32(-42).push_str_unescaped(" ");
            writer.push_bool(true).push_str_unescaped(" ");
            writer.push_f64(1.5);
        });
        assert_eq!(out, "-42 true 1.5");
    }

    #[test]
    fn push_view_splices_nested_views() {
        let mut inner_parts = ViewParts::new();
        PartsWriter::new(&mut inner_parts, HtmlContext::Text).push_str("a < b");
        let inner = View::new(inner_parts);

        let out = render(|parts| {
            PartsWriter::new(parts, HtmlContext::Unescaped).push_str("<p>");
            parts.push_view(inner);
            PartsWriter::new(parts, HtmlContext::Unescaped).push_str("</p>");
        });
        assert_eq!(out, "<p>a &lt; b</p>");
    }

    #[test]
    fn size_hint_is_exact_for_unescaped_strings() {
        let view = View::unescaped_unchecked("<b>raw</b>");
        assert_eq!(view.part.size_hint(), 10);
    }

    mod hydration {
        use topcoat_core::island::Islands;

        use super::*;

        /// A view of one element with a key on it and a marked dynamic child,
        /// the shape the `view!` macro produces for `<p>(value)</p>`.
        fn keyed_view() -> View {
            let mut parts = ViewParts::new();
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("<p");
            parts.push_hydration_site(HydrationSite::TemplateRoot);
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str(">");
            parts.push_hydration_site(HydrationSite::ChildStart);
            PartsWriter::new(&mut parts, HtmlContext::Text).push_str("value");
            parts.push_hydration_site(HydrationSite::ChildEnd);
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("</p>");
            View::new(parts)
        }

        #[test]
        fn the_wrapper_carries_what_a_client_needs_to_take_over() {
            let cx = Cx::default();
            let instance = cx.islands().next_instance();
            assert_eq!(
                keyed_view()
                    .island_element("counter", instance, "[5.0]")
                    .render(&cx),
                concat!(
                    r#"<topcoat-island data-ti="counter" data-tk="i0" data-ts="[5.0]">"#,
                    r#"<p data-hk="i0.0"><!--$-->value<!--/--></p>"#,
                    "</topcoat-island>",
                ),
            );
        }

        #[test]
        fn the_wrapper_is_the_tag_a_foreign_runtime_skips() {
            let cx = Cx::default();
            let html = View::empty()
                .island_element("x", cx.islands().next_instance(), "[]")
                .render(&cx);
            assert!(html.starts_with(&format!("<{ISLAND_TAG} ")), "{html}");
            assert!(html.ends_with(&format!("</{ISLAND_TAG}>")), "{html}");
        }

        #[test]
        fn a_seed_payload_cannot_end_its_attribute() {
            let cx = Cx::default();
            let html = View::empty()
                .island_element("x", cx.islands().next_instance(), r#"["a\"b"]"#)
                .render(&cx);
            assert!(
                html.contains(r#"data-ts="[&quot;a\&quot;b&quot;]""#),
                "{html}"
            );
        }

        #[test]
        fn hydration_sites_render_nothing_outside_an_island() {
            assert_eq!(keyed_view().render(&Cx::default()), "<p>value</p>");
        }

        #[test]
        fn an_island_renders_keys_and_markers() {
            let cx = Cx::default();
            let instance = cx.islands().next_instance();
            assert_eq!(
                keyed_view().island(instance).render(&cx),
                "<p data-hk=\"i0.0\"><!--$-->value<!--/--></p>",
            );
        }

        #[test]
        fn each_island_keys_against_its_own_instance() {
            let cx = Cx::default();
            let mut parts = ViewParts::new();
            parts.push_view(keyed_view().island(cx.islands().next_instance()));
            parts.push_view(keyed_view().island(cx.islands().next_instance()));

            let html = View::new(parts).render(&cx);
            assert!(html.contains("data-hk=\"i0.0\""), "{html}");
            assert!(html.contains("data-hk=\"i1.0\""), "{html}");
        }

        #[test]
        fn an_island_restores_the_surrounding_scope() {
            // Content after an island is outside it again, so it must go back
            // to writing no hydration sites at all.
            let cx = Cx::default();
            let mut parts = ViewParts::new();
            parts.push_view(keyed_view().island(cx.islands().next_instance()));
            parts.push_view(keyed_view());

            assert_eq!(
                View::new(parts).render(&cx),
                "<p data-hk=\"i0.0\"><!--$-->value<!--/--></p><p>value</p>",
            );
        }

        #[test]
        fn a_nested_island_keys_against_the_inner_instance() {
            let cx = Cx::default();
            let outer = cx.islands().next_instance();
            let inner = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("<div");
            parts.push_hydration_site(HydrationSite::TemplateRoot);
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str(">");
            parts.push_view(keyed_view().island(inner));
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("</div>");
            // The outer island carries on numbering where it left off: the inner
            // island's keys were never its to count.
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("<hr");
            parts.push_hydration_site(HydrationSite::TemplateRoot);
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str(">");

            assert_eq!(
                View::new(parts).island(outer).render(&cx),
                concat!(
                    "<div data-hk=\"i0.0\">",
                    "<p data-hk=\"i1.0\"><!--$-->value<!--/--></p>",
                    "</div><hr data-hk=\"i0.1\">",
                ),
            );
        }

        #[test]
        fn a_component_nests_its_keys_under_the_ordinal_the_call_took() {
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            // The island's own view claims ordinal 0, then calls a component,
            // which takes ordinal 1 and starts its own numbering from zero.
            parts.push_view(keyed_view());
            parts.push_view(keyed_view().component());
            // Back in the island's view, numbering carries on past the call.
            parts.push_view(keyed_view());

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                concat!(
                    "<p data-hk=\"i0.0\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.10\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.2\"><!--$-->value<!--/--></p>",
                ),
            );
        }

        #[test]
        fn a_component_inside_a_component_nests_again() {
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            parts.push_view(keyed_view());
            parts.push_view(keyed_view().component());
            let inner = View::new({
                let mut inner = ViewParts::new();
                inner.push_view(keyed_view());
                inner.push_view(keyed_view().component());
                inner
            });
            parts.push_view(inner.component());

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                concat!(
                    "<p data-hk=\"i0.0\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.10\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.20\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.210\"><!--$-->value<!--/--></p>",
                ),
            );
        }

        #[test]
        fn the_same_component_rendered_twice_writes_different_keys() {
            // The point of nesting: a component numbers its own keys from zero,
            // so without a context two calls would claim the same nodes.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            parts.push_view(keyed_view().component());
            parts.push_view(keyed_view().component());

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                concat!(
                    "<p data-hk=\"i0.00\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.10\"><!--$-->value<!--/--></p>",
                ),
            );
        }

        #[test]
        fn a_component_outside_an_island_takes_no_ordinal() {
            // Nothing is numbered outside an island, so wrapping a view as a
            // component there changes neither the markup nor the numbering of
            // the island that follows.
            let cx = Cx::default();
            let mut parts = ViewParts::new();
            parts.push_view(keyed_view().component());
            parts.push_view(keyed_view().island(cx.islands().next_instance()));

            assert_eq!(
                View::new(parts).render(&cx),
                concat!(
                    "<p>value</p>",
                    "<p data-hk=\"i0.0\"><!--$-->value<!--/--></p>",
                ),
            );
        }

        #[test]
        fn an_island_inside_a_component_keys_against_itself() {
            let cx = Cx::default();
            let outer = cx.islands().next_instance();
            let inner = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            parts.push_view(keyed_view());
            parts.push_view(
                View::new({
                    let mut body = ViewParts::new();
                    body.push_view(keyed_view().island(inner));
                    body
                })
                .component(),
            );
            parts.push_view(keyed_view());

            assert_eq!(
                View::new(parts).island(outer).render(&cx),
                concat!(
                    "<p data-hk=\"i0.0\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i1.0\"><!--$-->value<!--/--></p>",
                    "<p data-hk=\"i0.2\"><!--$-->value<!--/--></p>",
                ),
            );
        }

        #[test]
        fn a_component_past_ten_calls_keeps_the_segments_separable() {
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            for _ in 0..11 {
                parts.push_hydration_site(HydrationSite::TemplateRoot);
            }
            // The eleventh call takes ordinal 11, whose two digits the letter
            // records, so the key inside it cannot be read as any other.
            parts.push_view(keyed_view().component());

            let html = View::new(parts).island(instance).render(&cx);
            assert!(html.contains("data-hk=\"i0.a10\""), "{html}");
            assert!(html.contains("data-hk=\"i0.a110\""), "{html}");
        }

        /// The view of a component that renders its child content inside its own
        /// element, which is the shape `<div>(child)</div>` produces.
        fn wrapping_view(child: View) -> View {
            let mut parts = ViewParts::new();
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("<div");
            parts.push_hydration_site(HydrationSite::TemplateRoot);
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str(">");
            parts.push_view(View::child_content());
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("</div>");
            View::new(parts).component_with_child(child)
        }

        /// A view of one element with a key on it and nothing in it, the shape
        /// the child content of a component call is written as.
        fn empty_keyed_view() -> View {
            let mut parts = ViewParts::new();
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("<span");
            parts.push_hydration_site(HydrationSite::TemplateRoot);
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("></span>");
            View::new(parts)
        }

        #[test]
        fn child_content_is_numbered_in_the_callers_numbering() {
            // The oracle row: eager child content is built as an argument, so it
            // takes the caller's ordinal 0 and the component's own call takes 1.
            // The keys the render writes are therefore not in document order.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            assert_eq!(
                wrapping_view(empty_keyed_view())
                    .island(instance)
                    .render(&cx),
                "<div data-hk=\"i0.10\"><span data-hk=\"i0.0\"></span></div>",
            );
        }

        #[test]
        fn a_sibling_after_a_call_with_child_content_skips_both_ordinals() {
            // The second oracle row: the child spent one ordinal and the call
            // spent one, so the next sibling is numbered 2.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            parts.push_view(wrapping_view(empty_keyed_view()));
            parts.push_view(empty_keyed_view());

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                concat!(
                    "<div data-hk=\"i0.10\"><span data-hk=\"i0.0\"></span></div>",
                    "<span data-hk=\"i0.2\"></span>",
                ),
            );
        }

        #[test]
        fn child_content_a_view_never_renders_is_numbered_all_the_same() {
            // Building the argument is what spends the ordinals, and the client
            // builds it too, so a view that drops its child content must not
            // shift what follows.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            parts.push_view(empty_keyed_view().component_with_child(empty_keyed_view()));
            parts.push_view(empty_keyed_view());

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                concat!(
                    "<span data-hk=\"i0.10\"></span>",
                    "<span data-hk=\"i0.2\"></span>",
                ),
            );
        }

        #[test]
        fn child_content_that_calls_a_component_spends_the_callers_ordinals() {
            // The content is one component call, so it takes the caller's
            // ordinal 0 and numbers its own view under it, before the call it is
            // an argument of takes ordinal 1.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            assert_eq!(
                wrapping_view(empty_keyed_view().component())
                    .island(instance)
                    .render(&cx),
                "<div data-hk=\"i0.10\"><span data-hk=\"i0.00\"></span></div>",
            );
        }

        #[test]
        fn where_a_view_renders_its_child_content_does_not_change_its_keys() {
            // The content is numbered where the call is, and only its bytes wait
            // for the view to place them.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut inner = ViewParts::new();
            inner.push_view(View::child_content());
            inner.push_view(empty_keyed_view());
            let view = View::new(inner).component_with_child(empty_keyed_view());

            assert_eq!(
                view.island(instance).render(&cx),
                concat!(
                    "<span data-hk=\"i0.0\"></span>",
                    "<span data-hk=\"i0.10\"></span>",
                ),
            );
        }

        #[test]
        fn child_content_belongs_to_the_component_it_was_passed_to() {
            // A component called from inside another one has child content of
            // its own, and a view that renders content it was never given
            // renders nothing.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let outer = wrapping_view(empty_keyed_view());
            let mut parts = ViewParts::new();
            parts.push_view(outer);
            // A component with no child content at all, whose view still asks
            // for it.
            let mut asking = ViewParts::new();
            asking.push_view(View::child_content());
            asking.push_view(empty_keyed_view());
            parts.push_view(View::new(asking).component());

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                concat!(
                    "<div data-hk=\"i0.10\"><span data-hk=\"i0.0\"></span></div>",
                    "<span data-hk=\"i0.20\"></span>",
                ),
            );
        }

        #[test]
        fn child_content_outside_an_island_renders_without_keys() {
            let cx = Cx::default();
            assert_eq!(
                wrapping_view(empty_keyed_view()).render(&cx),
                "<div><span></span></div>",
            );
        }

        #[test]
        fn child_content_is_rendered_where_the_call_is_even_when_it_is_written_later() {
            // The aside buffer lifts the content's bytes out of the output, so
            // what surrounds the call is untouched by having rendered it.
            let cx = Cx::default();
            let instance = cx.islands().next_instance();

            let mut parts = ViewParts::new();
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("<p>");
            parts.push_view(wrapping_view(empty_keyed_view()));
            PartsWriter::new(&mut parts, HtmlContext::Unescaped).push_str("</p>");

            assert_eq!(
                View::new(parts).island(instance).render(&cx),
                "<p><div data-hk=\"i0.10\"><span data-hk=\"i0.0\"></span></div></p>",
            );
        }

        #[test]
        fn keys_are_numbered_in_the_order_they_render() {
            let islands = Islands::default();
            let instance = islands.next_instance();
            let mut parts = ViewParts::new();
            for _ in 0..3 {
                parts.push_hydration_site(HydrationSite::TemplateRoot);
            }

            assert_eq!(
                View::new(parts).island(instance).render(&Cx::default()),
                " data-hk=\"i0.0\" data-hk=\"i0.1\" data-hk=\"i0.2\"",
            );
        }

        #[test]
        fn markers_take_no_key_of_their_own() {
            // The client finds a marker by walking, not by key, so a marker
            // between two roots must not shift the numbering.
            let islands = Islands::default();
            let instance = islands.next_instance();
            let mut parts = ViewParts::new();
            parts.push_hydration_site(HydrationSite::TemplateRoot);
            parts.push_hydration_site(HydrationSite::ChildStart);
            parts.push_hydration_site(HydrationSite::ChildEnd);
            parts.push_hydration_site(HydrationSite::TemplateRoot);

            assert_eq!(
                View::new(parts).island(instance).render(&Cx::default()),
                " data-hk=\"i0.0\"<!--$--><!--/--> data-hk=\"i0.1\"",
            );
        }

        #[test]
        fn an_island_past_ten_templates_keeps_writing_the_clients_keys() {
            let islands = Islands::default();
            let instance = islands.next_instance();
            let mut parts = ViewParts::new();
            for _ in 0..12 {
                parts.push_hydration_site(HydrationSite::TemplateRoot);
            }

            // The client's own numbering starts carrying the ordinal's length at
            // ten, and only the client's spelling is findable.
            let html = View::new(parts).island(instance).render(&Cx::default());
            assert!(html.ends_with(" data-hk=\"i0.9\" data-hk=\"i0.a10\" data-hk=\"i0.a11\""));
            assert_eq!(html.matches("data-hk").count(), 12);
        }
    }

    #[cfg(feature = "http")]
    mod response {
        use http::header::{CACHE_CONTROL, SET_COOKIE};
        use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};

        use super::*;
        use crate::NodeViewParts;

        fn push_node(parts: &mut ViewParts, value: impl NodeViewParts) {
            value.into_view_parts(
                &Cx::default(),
                &mut PartsWriter::new(parts, HtmlContext::Text),
            );
        }

        fn push_text(parts: &mut ViewParts, text: &'static str) {
            PartsWriter::new(parts, HtmlContext::Text).push_str(text);
        }

        #[test]
        fn status_code_is_recorded_and_renders_nothing() {
            let mut parts = ViewParts::new();
            push_text(&mut parts, "a");
            push_node(&mut parts, StatusCode::NOT_FOUND);
            push_text(&mut parts, "b");

            let rendered = View::new(parts).render_response(&Cx::default());
            assert_eq!(rendered.html, "ab");
            assert_eq!(rendered.status_code, Some(StatusCode::NOT_FOUND));
            assert!(rendered.headers.is_empty());
        }

        #[test]
        fn render_response_without_declarations_is_empty() {
            let mut parts = ViewParts::new();
            push_text(&mut parts, "a");

            let rendered = View::new(parts).render_response(&Cx::default());
            assert_eq!(rendered.html, "a");
            assert_eq!(rendered.status_code, None);
            assert!(rendered.headers.is_empty());
        }

        #[test]
        fn render_discards_declarations() {
            let mut parts = ViewParts::new();
            push_node(&mut parts, StatusCode::NOT_FOUND);
            push_node(
                &mut parts,
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            );
            push_text(&mut parts, "a");

            assert_eq!(View::new(parts).render(&Cx::default()), "a");
        }

        #[test]
        fn first_status_code_wins() {
            let mut parts = ViewParts::new();
            push_node(&mut parts, StatusCode::NOT_FOUND);
            push_node(&mut parts, StatusCode::OK);

            let rendered = View::new(parts).render_response(&Cx::default());
            assert_eq!(rendered.status_code, Some(StatusCode::NOT_FOUND));
        }

        #[test]
        fn first_mention_of_a_header_name_wins() {
            let mut parts = ViewParts::new();
            push_node(
                &mut parts,
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            );
            let mut later = HeaderMap::new();
            later.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=60"));
            later.insert(
                HeaderName::from_static("x-extra"),
                HeaderValue::from_static("1"),
            );
            push_node(&mut parts, later);

            let rendered = View::new(parts).render_response(&Cx::default());
            assert_eq!(rendered.headers[CACHE_CONTROL], "no-store");
            assert_eq!(rendered.headers["x-extra"], "1");
        }

        #[test]
        fn one_map_keeps_all_values_for_a_name() {
            let mut first = HeaderMap::new();
            first.append(SET_COOKIE, HeaderValue::from_static("a=1"));
            first.append(SET_COOKIE, HeaderValue::from_static("b=2"));
            let mut later = HeaderMap::new();
            later.insert(SET_COOKIE, HeaderValue::from_static("c=3"));

            let mut parts = ViewParts::new();
            push_node(&mut parts, first);
            push_node(&mut parts, later);

            let rendered = View::new(parts).render_response(&Cx::default());
            let cookies: Vec<_> = rendered.headers.get_all(SET_COOKIE).iter().collect();
            assert_eq!(cookies, ["a=1", "b=2"]);
        }

        #[test]
        fn placement_decides_precedence_across_nested_views() {
            let mut inner_parts = ViewParts::new();
            push_node(&mut inner_parts, StatusCode::NOT_FOUND);
            push_text(&mut inner_parts, "inner");
            let inner = View::new(inner_parts);

            // A status code before the nested view overrides it.
            let mut outer_parts = ViewParts::new();
            push_node(&mut outer_parts, StatusCode::FORBIDDEN);
            outer_parts.push_view(inner.clone());
            let rendered = View::new(outer_parts).render_response(&Cx::default());
            assert_eq!(rendered.status_code, Some(StatusCode::FORBIDDEN));

            // A status code after the nested view is only a fallback.
            let mut outer_parts = ViewParts::new();
            outer_parts.push_view(inner);
            push_node(&mut outer_parts, StatusCode::FORBIDDEN);
            let rendered = View::new(outer_parts).render_response(&Cx::default());
            assert_eq!(rendered.status_code, Some(StatusCode::NOT_FOUND));
        }
    }
}
