#[cfg(feature = "http")]
use http::{HeaderMap, StatusCode};
use topcoat_core::island::{IslandInstance, KeyContext};

/// A plain string writer that render output accumulates into.
///
/// `Formatter` is escaping-agnostic: [`write_str`](Self::write_str) and
/// [`write_char`](Self::write_char) append exactly what they are given. Text
/// that needs to be made safe for an HTML position is written through an
/// [`HtmlWriter`](crate::HtmlWriter) created for the matching
/// [`HtmlContext`](crate::HtmlContext) instead.
///
/// Alongside the output it carries the state a render collects as it goes: the
/// response metadata a view declared, and the island the current part belongs
/// to.
pub struct Formatter<'a> {
    buf: &'a mut String,
    island: Island,
    #[cfg(feature = "http")]
    status_code: Option<StatusCode>,
    #[cfg(feature = "http")]
    headers: HeaderMap,
}

impl<'a> Formatter<'a> {
    /// Creates a new `Formatter` that writes into the given destination.
    #[inline]
    pub fn new(buf: &'a mut String) -> Self {
        Self {
            buf,
            island: Island::default(),
            #[cfg(feature = "http")]
            status_code: None,
            #[cfg(feature = "http")]
            headers: HeaderMap::new(),
        }
    }

    /// The island instance the part being rendered belongs to, or `None`
    /// outside every island.
    ///
    /// Hydration keys and markers are written only inside an island, so a
    /// render that never enters one produces the same bytes it would if
    /// hydration did not exist.
    #[inline]
    pub(crate) fn island(&self) -> Option<IslandInstance> {
        self.island.instance
    }

    /// The ordinal for the next hydration key written inside the current
    /// island.
    ///
    /// Keys are numbered as they are written, which is the order the client
    /// claims the nodes they name: the client counts its own keys off as it
    /// walks, so the two agree only if the server counts the same way. Numbering
    /// them anywhere earlier than here would number the branches a render did
    /// not take.
    #[inline]
    fn next_key(&mut self) -> u32 {
        let ordinal = self.island.next_key;
        self.island.next_key += 1;
        ordinal
    }

    /// Writes the next hydration key of `instance`, in the component nesting the
    /// render is inside.
    pub(crate) fn write_next_key(&mut self, instance: IslandInstance) {
        let ordinal = self.next_key();
        instance
            .write_key_in(&mut *self.buf, &self.island.context, ordinal)
            .expect("writing into a string cannot fail");
    }

    /// Writes the child content the component being rendered was called with.
    ///
    /// The content was rendered before the component was entered, so writing it
    /// here writes bytes that are already numbered. A component that was called
    /// with no child content writes nothing.
    pub(crate) fn write_child_content(&mut self) {
        if let Some(content) = &self.island.child_content {
            self.buf.push_str(content);
        }
    }

    /// Renders through `render` into a buffer of its own, returning what it
    /// wrote.
    ///
    /// The render goes through this formatter, so it numbers its keys where the
    /// numbering has got to and records response metadata in the order it ran;
    /// only its bytes are lifted out, to be written somewhere else later.
    pub(crate) fn write_aside(&mut self, render: impl FnOnce(&mut Self)) -> String {
        let mark = self.buf.len();
        render(self);
        self.buf.split_off(mark)
    }

    /// Makes `instance` the island the following parts belong to, returning the
    /// state it replaced so the caller can restore it.
    ///
    /// Key numbering travels with the island, so an island nested in another
    /// numbers its own keys from zero and the outer one carries on where it left
    /// off: the inner island's keys were never the outer one's to count.
    #[inline]
    pub(crate) fn enter_island(&mut self, instance: IslandInstance) -> Island {
        std::mem::replace(
            &mut self.island,
            Island {
                instance: Some(instance),
                next_key: 0,
                context: KeyContext::ROOT,
                child_content: None,
            },
        )
    }

    /// Makes the following parts a component's own, returning the state it
    /// replaced so the caller can restore it.
    ///
    /// A component's view numbers its keys from zero, so the call takes the next
    /// ordinal the caller had reached and every key the component writes is
    /// nested under it. Nesting happens before the component renders, which is
    /// where the client opens its own context, so the two number the same nodes
    /// the same way.
    ///
    /// Outside an island no key is written at all, so a component call there
    /// takes no ordinal and opens no context.
    ///
    /// `child_content` is the already-rendered child content the component was
    /// called with, which its view writes wherever it renders it. It belongs to
    /// the component being entered alone: a component called from inside this one
    /// replaces it with its own, and a view that renders child content it was
    /// never given writes nothing.
    pub(crate) fn enter_component(&mut self, child_content: Option<String>) -> Island {
        let context = if self.island.instance.is_none() {
            self.island.context.clone()
        } else {
            let ordinal = self.next_key();
            self.island.context.enter(ordinal)
        };
        let nested = Island {
            instance: self.island.instance,
            next_key: 0,
            context,
            child_content,
        };
        std::mem::replace(&mut self.island, nested)
    }

    /// Restores the state an [`enter_island`](Self::enter_island) or an
    /// [`enter_component`](Self::enter_component) replaced.
    #[inline]
    pub(crate) fn restore(&mut self, island: Island) {
        self.island = island;
    }

    /// Writes a string verbatim.
    #[inline]
    pub fn write_str(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    /// Writes a single character verbatim.
    #[inline]
    pub fn write_char(&mut self, c: char) {
        self.buf.push(c);
    }

    /// Records a response status code, keeping an earlier one if already
    /// recorded: the first status code rendered wins.
    #[cfg(feature = "http")]
    #[inline]
    pub(crate) fn record_status_code(&mut self, status_code: StatusCode) {
        self.status_code.get_or_insert(status_code);
    }

    /// Records response headers, keeping earlier values for names already
    /// recorded: the first render part that mentions a header name provides
    /// all of that name's values.
    #[cfg(feature = "http")]
    pub(crate) fn record_headers(&mut self, headers: &HeaderMap) {
        if self.headers.is_empty() {
            self.headers = headers.clone();
            return;
        }
        for name in headers.keys() {
            if !self.headers.contains_key(name) {
                for value in headers.get_all(name) {
                    self.headers.append(name.clone(), value.clone());
                }
            }
        }
    }

    /// Consumes the formatter, returning the recorded status code and
    /// headers.
    #[cfg(feature = "http")]
    pub(crate) fn into_recorded(self) -> (Option<StatusCode>, HeaderMap) {
        (self.status_code, self.headers)
    }
}

impl std::fmt::Write for Formatter<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        Formatter::write_str(self, s);
        Ok(())
    }

    fn write_char(&mut self, c: char) -> std::fmt::Result {
        Formatter::write_char(self, c);
        Ok(())
    }
}

/// The render state an island or a component entry replaces: the island a render
/// is inside, how far its key numbering has got, the component nesting the keys
/// it writes belong to, and the child content the component was called with.
#[derive(Debug, Clone, Default)]
pub(crate) struct Island {
    instance: Option<IslandInstance>,
    next_key: u32,
    context: KeyContext,
    child_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_str_is_verbatim() {
        let mut buf = String::new();
        let mut f = Formatter::new(&mut buf);
        f.write_str("<b>&\"'</b>");
        assert_eq!(buf, "<b>&\"'</b>");
    }

    #[test]
    fn write_char_is_verbatim() {
        let mut buf = String::new();
        let mut f = Formatter::new(&mut buf);
        f.write_char('<');
        f.write_char('é');
        assert_eq!(buf, "<é");
    }

    #[test]
    fn fmt_write_is_verbatim() {
        use std::fmt::Write;

        let (one, two) = (1, "two");
        let mut buf = String::new();
        let mut f = Formatter::new(&mut buf);
        write!(f, "{one} < {two}").unwrap();
        assert_eq!(buf, "1 < two");
    }
}
