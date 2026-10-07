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
//! # The walk
//!
//! [`nesting_within_limits`] walks the graph pdf-extract would walk,
//! resolving names the way it does, without running any of it. For every
//! page it follows the `Parent` chain to the page's resources, reads the
//! names the page content invokes with `Do`, and descends into each form
//! those names resolve to, and into the forms that form invokes. It refuses
//! the document when:
//!
//! - a form is reached while it is still open, which is a cycle;
//! - a form is nested deeper than [`MAX_FORM_DEPTH`];
//! - a `Parent` chain loops or is longer than [`MAX_PARENT_CHAIN`];
//! - the forms run more than [`MAX_FORM_RUNS`] times, or over more than
//!   [`MAX_FORM_CONTENT_BYTES`] of content, all pages together.
//!
//! The first three bound the stack. The last bounds time: forms that each
//! invoke the next one twice do end, but a chain of them runs its last form
//! an exponential number of times.
//!
//! Where pdf-extract would panic instead of recursing (a page that is not a
//! dictionary, content that does not parse, a `Do` without resources), the
//! walk has nothing to follow and lets the document through: that panic is
//! contained by the caller.
//!
//! # Cost of the walk itself
//!
//! A form's content is decoded once and reduced to the names it invokes and
//! its length, so a form that runs many times is counted, not decoded again.
//! The walk recurses once per nesting level and stops past
//! [`MAX_FORM_DEPTH`], and it follows a `Parent` chain in a loop, not by
//! recursion. It runs last in
//! [`within_budget`](crate::documents::pdf_budget::within_budget), so every
//! stream it decodes has already been measured against the size budget.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// The same lopdf as pdf-extract uses; see `analyze`.
use pdf_extract as lopdf;

/// Deepest a form may be nested below its page. Real statements nest a
/// logo or a stamp one or two levels deep; each level is a pdf-extract
/// stack frame on the extraction thread.
///
/// A test in `analyze` reads a document nested this deep on a quarter of
/// the extraction thread's stack, in a debug build. The reason for 16 in
/// particular is not recorded.
pub(super) const MAX_FORM_DEPTH: usize = 16;

/// Most form runs a whole document may expand to, every page together.
///
/// This bounds time, not the stack. The reason for 20 000 in particular is
/// not recorded; `forms_that_fan_out_past_the_run_limit_are_refused` pins
/// that 2^15 runs are too many.
pub(super) const MAX_FORM_RUNS: usize = 20_000;

/// Most form content pdf-extract may parse, every run together. A form is
/// decoded and parsed again each time it runs.
///
/// The same figure as the budget on decoded streams
/// (`MAX_PDF_DECODED_BYTES`), which counts each stream once; this one counts
/// a form once per run. `form_content_is_counted_once_per_run_up_to_the_limit`
/// pins it.
pub(super) const MAX_FORM_CONTENT_BYTES: usize = 32 * 1024 * 1024;

/// Most dictionaries on the way from a page to the root of the page tree,
/// the page included.
///
/// pdf-extract recurses once per link, so this bounds stack as well. A real
/// page tree is a few levels deep. The reason for 64 in particular is not
/// recorded; `a_parent_chain_is_accepted_up_to_the_limit_and_refused_past_it`
/// pins it.
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
    /// The document being walked.
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
    /// Walks one page: its `Parent` chain, then every form its content invokes.
    ///
    /// A page pdf-extract cannot run at all (not a dictionary, no readable
    /// content, no resources) is accepted without a walk.
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
        while let Some(dictionary) = node {
            if seen.len() == MAX_PARENT_CHAIN || !seen.insert(std::ptr::from_ref(dictionary)) {
                return Err(Unbounded);
            }
            if resources.is_none() {
                resources = dictionary
                    .get(b"Resources")
                    .ok()
                    .and_then(|object| self.resolve(object))
                    .and_then(|object| object.as_dict().ok());
            }
            node = dictionary
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

    /// What `form` invokes and how long its content is, decoded on first use
    /// and remembered by the stream's address.
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
///
/// The decode is not capped here. The caller has already measured every
/// stream of the document against the size budget.
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
    pub(in crate::documents) fn pdf_with_forms(page: &[usize], forms: &[Vec<usize>]) -> Vec<u8> {
        use lopdf::{Dictionary, Document, Object, Stream, dictionary};

        /// Content that runs each of `forms` once.
        fn invoking(forms: &[usize]) -> Vec<u8> {
            let mut content = Vec::new();
            for form in forms {
                content.extend_from_slice(format!("/F{form} Do\n").as_bytes());
            }
            content
        }

        /// The content of a form that runs `forms`, or draws the leaf text
        /// when it runs none.
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

    /// Whether the PDF in `data` passes the nesting check.
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

    /// A document whose one page sits under `ancestors` nested `Pages`
    /// nodes, so its `Parent` chain is `ancestors + 1` dictionaries long.
    fn page_under(ancestors: usize) -> lopdf::Document {
        use lopdf::{Document, Object, dictionary};

        let mut doc = Document::with_version("1.5");
        let nodes: Vec<lopdf::ObjectId> = (0..ancestors).map(|_| doc.new_object_id()).collect();
        let lowest = *nodes.last().expect("a page needs a parent");
        let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => lowest });

        let mut parent = None;
        let mut kids = nodes.iter().skip(1).copied().chain([page_id]);
        for node in &nodes {
            let kid = kids.next().expect("one kid per node");
            let mut pages = dictionary! {
                "Type" => "Pages",
                "Kids" => vec![kid.into()],
                "Count" => 1,
            };
            if let Some(parent) = parent {
                pages.set("Parent", Object::Reference(parent));
            }
            doc.objects.insert(*node, Object::Dictionary(pages));
            parent = Some(*node);
        }

        let root = *nodes.first().expect("a page needs a parent");
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    #[test]
    fn a_parent_chain_is_accepted_up_to_the_limit_and_refused_past_it() {
        let at_the_limit = page_under(MAX_PARENT_CHAIN - 1);
        let past_the_limit = page_under(MAX_PARENT_CHAIN);

        // Both pages are found, so both chains are walked.
        assert_eq!(at_the_limit.get_pages().len(), 1);
        assert_eq!(past_the_limit.get_pages().len(), 1);

        assert!(nesting_within_limits(&at_the_limit));
        assert!(!nesting_within_limits(&past_the_limit));
    }

    /// A document whose page runs one form `runs` times, the form holding
    /// `form_bytes` bytes of content that draws nothing.
    fn page_running_a_form(runs: usize, form_bytes: usize) -> lopdf::Document {
        use lopdf::{Dictionary, Document, Object, Stream, dictionary};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();

        let form = doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form" },
            vec![b' '; form_bytes],
        ));
        let contents = doc.add_object(Stream::new(
            Dictionary::new(),
            "/F0 Do\n".repeat(runs).into_bytes(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => contents,
            "Resources" => dictionary! { "XObject" => dictionary! { "F0" => form } },
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
        doc
    }

    #[test]
    fn form_content_is_counted_once_per_run_up_to_the_limit() {
        const FORM_BYTES: usize = 1024 * 1024;
        let runs_at_the_limit = MAX_FORM_CONTENT_BYTES / FORM_BYTES;
        assert_eq!(runs_at_the_limit * FORM_BYTES, MAX_FORM_CONTENT_BYTES);
        assert!(runs_at_the_limit < MAX_FORM_RUNS);

        // One megabyte of form, stored once: the count is per run.
        assert!(nesting_within_limits(&page_running_a_form(
            runs_at_the_limit,
            FORM_BYTES
        )));
        assert!(!nesting_within_limits(&page_running_a_form(
            runs_at_the_limit + 1,
            FORM_BYTES
        )));
        // One byte over, in a single run of a larger form.
        assert!(!nesting_within_limits(&page_running_a_form(
            1,
            MAX_FORM_CONTENT_BYTES + 1
        )));
    }
}
