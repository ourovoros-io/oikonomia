//! Refuses PDFs that would make pdf-extract recurse without end.
//!
//! pdf-extract runs a form `XObject` by calling itself on the form's content
//! each time a page or another form invokes it with `Do`. It keeps no depth
//! limit and no record of the forms it is already inside, so a form that
//! invokes itself, directly or through other forms, recurses until the
//! thread's stack overflows. pdf-extract also looks up a page's `Resources`
//! and `MediaBox` by recursing up its `Parent` links, so a page whose
//! `Parent` chain loops never finds the root either. A stack overflow aborts
//! the process; `catch_unwind` cannot catch it, so these documents must be
//! refused before pdf-extract sees them.
//!
//! [`nesting_within_limits`] walks the graph pdf-extract would walk, resolving
//! names the way it does, without running any of it. A third bound is about
//! time, not the stack: forms that each invoke the next one twice do end, but
//! a chain of them runs its last form an exponential number of times.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// The same lopdf as pdf-extract uses; see `analyze`.
use pdf_extract as lopdf;

/// Deepest a form may be nested below its page. Real statements nest a
/// logo or a stamp one or two levels deep; each level is a pdf-extract
/// stack frame on the extraction thread.
pub(super) const MAX_FORM_DEPTH: usize = 16;

/// Most form runs a whole document may expand to, every page together.
pub(super) const MAX_FORM_RUNS: usize = 20_000;

/// Most form content pdf-extract may parse, every run together. A form is
/// decoded and parsed again each time it runs.
pub(super) const MAX_FORM_CONTENT_BYTES: usize = 32 * 1024 * 1024;

/// Longest `Parent` chain from a page to the root of the page tree.
const MAX_PARENT_CHAIN: usize = 64;

/// The walk found a cycle, or a chain or expansion past its limit.
#[derive(Debug)]
struct Unbounded;

/// Whether pdf-extract's recursion over `document` ends within the limits:
/// no form invokes itself, no form is nested deeper than
/// [`MAX_FORM_DEPTH`], all pages together run forms at most
/// [`MAX_FORM_RUNS`] times over at most [`MAX_FORM_CONTENT_BYTES`] of
/// content, and every page's `Parent` chain reaches the root.
///
/// `document` must be decrypted the way it will be extracted, since the walk
/// reads its content streams.
pub(super) fn nesting_within_limits(document: &lopdf::Document) -> bool {
    let mut walk = Walk {
        document,
        forms: HashMap::new(),
        open: HashSet::new(),
        runs: 0,
        content_bytes: 0,
    };
    document
        .get_pages()
        .into_values()
        .all(|page_id| walk.page(page_id).is_ok())
}

/// State of one walk over a document.
struct Walk<'a> {
    document: &'a lopdf::Document,
    /// What each form holds, decoded once.
    forms: HashMap<*const lopdf::Stream, Rc<Form>>,
    /// The forms between the page and the current one. Forms are told apart
    /// by address, which also covers a form written inline rather than as a
    /// numbered object.
    open: HashSet<*const lopdf::Stream>,
    /// Form runs so far, every page together.
    runs: usize,
    /// Bytes of form content those runs parse.
    content_bytes: usize,
}

/// What the walk needs of one form's content.
struct Form {
    /// The names it invokes with `Do`, in order.
    invoked: Vec<Vec<u8>>,
    /// Its length as pdf-extract parses it.
    content_len: usize,
}

impl<'a> Walk<'a> {
    fn page(&mut self, page_id: lopdf::ObjectId) -> Result<(), Unbounded> {
        // pdf-extract panics on a page that is not a dictionary; that panic
        // is contained, so there is nothing to walk.
        let Ok(page) = self.document.get_dictionary(page_id) else {
            return Ok(());
        };
        let resources = self.inherited_resources(page)?;
        let Ok(content) = self.document.get_page_content(page_id) else {
            return Ok(());
        };
        // Without resources pdf-extract cannot resolve a `Do` and panics.
        let Some(resources) = resources else {
            return Ok(());
        };
        let names = invoked_names(&content);
        self.run(&names, resources, 1)
    }

    /// The `Resources` pdf-extract would give `page`: the nearest in its
    /// `Parent` chain. Fails when that chain loops or is longer than
    /// [`MAX_PARENT_CHAIN`], found or not, since pdf-extract follows the
    /// whole chain for `MediaBox` as well.
    fn inherited_resources(
        &self,
        page: &'a lopdf::Dictionary,
    ) -> Result<Option<&'a lopdf::Dictionary>, Unbounded> {
        let mut seen = HashSet::new();
        let mut resources = None;
        let mut node = Some(page);
        while let Some(dict) = node {
            if seen.len() == MAX_PARENT_CHAIN || !seen.insert(std::ptr::from_ref(dict)) {
                return Err(Unbounded);
            }
            if resources.is_none() {
                resources = dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|object| self.resolve(object))
                    .and_then(|object| object.as_dict().ok());
            }
            node = dict
                .get(b"Parent")
                .and_then(lopdf::Object::as_reference)
                .and_then(|id| self.document.get_dictionary(id))
                .ok();
        }
        Ok(resources)
    }

    /// Walks the forms that `names` invoke, `depth` levels below the page.
    fn run(
        &mut self,
        names: &[Vec<u8>],
        resources: &'a lopdf::Dictionary,
        depth: usize,
    ) -> Result<(), Unbounded> {
        for name in names {
            let Some(form) = self.xobject(resources, name) else {
                continue;
            };
            let key = std::ptr::from_ref(form);
            let inner = self.form(form);
            self.runs += 1;
            self.content_bytes = self.content_bytes.saturating_add(inner.content_len);
            if depth > MAX_FORM_DEPTH
                || self.runs > MAX_FORM_RUNS
                || self.content_bytes > MAX_FORM_CONTENT_BYTES
                || !self.open.insert(key)
            {
                return Err(Unbounded);
            }
            // As in pdf-extract, a form without resources of its own uses
            // those of whatever invoked it.
            let form_resources = form
                .dict
                .get(b"Resources")
                .ok()
                .and_then(|object| self.resolve(object))
                .and_then(|object| object.as_dict().ok())
                .unwrap_or(resources);
            self.run(&inner.invoked, form_resources, depth + 1)?;
            self.open.remove(&key);
        }
        Ok(())
    }

    /// The stream `name` refers to in the `XObject` dictionary of
    /// `resources`. pdf-extract runs whatever stream it finds there as
    /// content, form or not.
    fn xobject(&self, resources: &'a lopdf::Dictionary, name: &[u8]) -> Option<&'a lopdf::Stream> {
        let xobjects = self
            .resolve(resources.get(b"XObject").ok()?)?
            .as_dict()
            .ok()?;
        self.resolve(xobjects.get(name).ok()?)?.as_stream().ok()
    }

    /// Follows one reference, as pdf-extract does.
    fn resolve(&self, object: &'a lopdf::Object) -> Option<&'a lopdf::Object> {
        match object {
            lopdf::Object::Reference(id) => self.document.get_object(*id).ok(),
            direct => Some(direct),
        }
    }

    fn form(&mut self, form: &'a lopdf::Stream) -> Rc<Form> {
        let entry = self
            .forms
            .entry(std::ptr::from_ref(form))
            .or_insert_with(|| {
                let content = stream_content(form);
                Rc::new(Form {
                    invoked: invoked_names(&content),
                    content_len: content.len(),
                })
            });
        Rc::clone(entry)
    }
}

/// The content of a form as pdf-extract reads it: decoded when lopdf can
/// decode it, as stored otherwise.
fn stream_content(stream: &lopdf::Stream) -> Cow<'_, [u8]> {
    if stream.filters().is_ok()
        && let Ok(decoded) = stream.decompressed_content()
    {
        return Cow::Owned(decoded);
    }
    Cow::Borrowed(&stream.content)
}

/// The names that `content` invokes with `Do`, in order. Content that does
/// not parse invokes nothing: pdf-extract panics on it before running any.
fn invoked_names(content: &[u8]) -> Vec<Vec<u8>> {
    let Ok(content) = lopdf::content::Content::decode(content) else {
        return Vec::new();
    };
    content
        .operations
        .into_iter()
        .filter(|operation| operation.operator == "Do")
        .filter_map(|operation| {
            operation
                .operands
                .first()
                .and_then(|operand| operand.as_name().ok())
                .map(<[u8]>::to_vec)
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// What a form that runs no other form draws, so a test can tell the
    /// deepest form was read.
    pub(in crate::documents) const LEAF_TEXT: &str = "leaf";

    /// A one-page PDF with forms `F0`, `F1`, … sharing one resource
    /// dictionary with the page. The page runs the forms in `page`, and form
    /// `i` runs the forms in `forms[i]`; a form that runs none draws
    /// [`LEAF_TEXT`].
    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
    pub(in crate::documents) fn pdf_with_forms(page: &[usize], forms: &[Vec<usize>]) -> Vec<u8> {
        use lopdf::{Dictionary, Document, Object, Stream, dictionary};

        fn invoking(forms: &[usize]) -> Vec<u8> {
            let mut content = Vec::new();
            for form in forms {
                content.extend_from_slice(format!("/F{form} Do\n").as_bytes());
            }
            content
        }

        fn form_content(forms: &[usize]) -> Vec<u8> {
            if forms.is_empty() {
                format!("BT /Helv 12 Tf 10 10 Td ({LEAF_TEXT}) Tj ET\n").into_bytes()
            } else {
                invoking(forms)
            }
        }

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources_id = doc.new_object_id();

        let mut xobjects = Dictionary::new();
        for (index, invoked) in forms.iter().enumerate() {
            let form = doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
                    "Resources" => resources_id,
                },
                form_content(invoked),
            ));
            xobjects.set(format!("F{index}"), form);
        }
        doc.objects.insert(
            resources_id,
            Object::Dictionary(dictionary! {
                "XObject" => xobjects,
                "Font" => dictionary! {
                    "Helv" => dictionary! {
                        "Type" => "Font",
                        "Subtype" => "Type1",
                        "BaseFont" => "Helvetica",
                    },
                },
            }),
        );

        let contents = doc.add_object(Stream::new(Dictionary::new(), invoking(page)));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => contents,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialize test pdf");
        bytes
    }

    #[expect(clippy::expect_used, reason = "test fails loudly by design")]
    fn within_limits(data: &[u8]) -> bool {
        nesting_within_limits(&lopdf::Document::load_mem(data).expect("load test pdf"))
    }

    /// Form `i` runs form `i + 1`, `depth` forms in all.
    pub(in crate::documents) fn chain(depth: usize) -> Vec<Vec<usize>> {
        (0..depth)
            .map(|form| {
                if form + 1 < depth {
                    vec![form + 1]
                } else {
                    vec![]
                }
            })
            .collect()
    }

    #[test]
    fn a_form_that_runs_itself_through_its_callers_resources_is_refused() {
        // The fixture's form has no resources of its own, so pdf-extract runs
        // it with the page's, where its name points back at it.
        let pdf = include_bytes!("../../testdata/hostile/xobject_self_loop.pdf");

        assert!(!within_limits(pdf));
    }

    #[test]
    fn forms_that_run_each_other_are_refused() {
        assert!(!within_limits(&pdf_with_forms(&[0], &[vec![1], vec![0]])));
    }

    #[test]
    fn a_page_whose_parent_chain_loops_is_refused() {
        let pdf = include_bytes!("../../testdata/hostile/page_parent_loop.pdf");

        assert!(!within_limits(pdf));
    }

    #[test]
    fn a_form_used_many_times_without_a_loop_is_accepted() {
        // A logo placed three times, each made of the same mark twice: reuse
        // is not a cycle.
        let pdf = pdf_with_forms(&[0, 0, 0], &[vec![1, 1], vec![]]);

        assert!(within_limits(&pdf));
    }

    #[test]
    fn forms_nested_to_the_depth_limit_are_accepted() {
        // That extracting them fits the extraction thread's stack is tested
        // in `analyze`, through the path the app takes.
        assert!(within_limits(&pdf_with_forms(&[0], &chain(MAX_FORM_DEPTH))));
    }

    #[test]
    fn forms_nested_past_the_depth_limit_are_refused() {
        assert!(!within_limits(&pdf_with_forms(
            &[0],
            &chain(MAX_FORM_DEPTH + 1)
        )));
    }

    #[test]
    fn forms_that_fan_out_past_the_run_limit_are_refused() {
        // Each form runs the next twice: no loop and shallow, but the last
        // form would run 2^15 times.
        let forms: Vec<Vec<usize>> = (0..15)
            .map(|form| if form < 14 { vec![form + 1; 2] } else { vec![] })
            .collect();
        assert!(2_usize.pow(15) > MAX_FORM_RUNS);

        assert!(!within_limits(&pdf_with_forms(&[0], &forms)));
    }

    #[test]
    fn a_page_without_forms_is_accepted() {
        let pdf = include_bytes!("../../testdata/documents/synthetic/pdf/english_total.pdf");

        assert!(within_limits(pdf));
    }
}
