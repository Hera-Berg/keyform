#![allow(clippy::unwrap_used)]
//! New art paths in a project that has none (the built-in prototype),
//! saved and reloaded. spec: 70-advanced/art-path.md §1, §5

use keyform_formats::cmo3::Cmo3Document;

#[test]
fn new_art_path_in_a_blank_model() {
    let mut doc = Cmo3Document::new_model("blank", 800, 600).unwrap();
    let mut doc = Cmo3Document::read(&doc.save_bytes().unwrap()).unwrap();
    let root = doc.model.root_part;
    let d = keyform_edit::artpath::Defaults {
        width: 6.0,
        opacity: 0.8,
        color: [0.2, 0.4, 0.6, 1.0],
    };
    let k =
        keyform_edit::artpath::create(&mut doc.model, root, &[[100.0, 100.0], [400.0, 300.0]], d)
            .unwrap();
    std::sync::Arc::make_mut(&mut doc.model.art_paths[k]).closed = false;
    let bytes = doc.save_bytes().unwrap();
    let back = Cmo3Document::read(&bytes).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.model.art_paths.len(), 1);
    assert_eq!(
        back.model.art_paths[0].grid.keyforms,
        doc.model.art_paths[0].grid.keyforms
    );
    assert_eq!(back.model.art_paths[0].info.parent_part, root);
    // The saved file evaluates to a visible stroke.
    let f = keyform_eval::Evaluator::new(&back.model).evaluate(&[]);
    assert!(f.art_paths[0].is_drawn());
}
