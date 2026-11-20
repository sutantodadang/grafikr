use grafikr_bridge::collections::{
    Collections, DataRequest, MAX_CACHE_ROWS_PER_NODE, MAX_REQUEST_ROWS, TreeItem, TreeSnapshot,
};
use grafikr_bridge::handles::Handle;
use serde_json::json;

#[test]
fn virtual_data_is_bounded_revisioned_and_stale_responses_do_not_mutate_cache() {
    let collections = Collections::new();
    let node = Handle(10);
    collections
        .create(node, 50_000, 20.0, vec!["value".into()])
        .unwrap();
    let first = collections
        .request_visible(node, 100, 20, "r1")
        .unwrap()
        .unwrap();
    assert_eq!(first.start, 80);
    assert_eq!(first.count, 60);
    assert!(first.count <= MAX_REQUEST_ROWS);
    let wrong = DataRequest {
        revision: first.revision + 1,
        ..first.clone()
    };
    assert_eq!(
        collections
            .provide(&wrong, vec![vec![json!(1)]; first.count as usize])
            .unwrap_err()
            .code,
        "stale_data"
    );
    collections
        .provide(&first, (0..first.count).map(|i| vec![json!(i)]).collect())
        .unwrap();
    assert!(
        collections
            .request_visible(node, 100, 20, "cached")
            .unwrap()
            .is_none()
    );
    assert_eq!(collections.cache_rows(node), first.count as usize);
    assert_eq!(&*collections.row(node, 80).unwrap(), &[json!(0)]);
    let revision = collections
        .update(node, 50_000, 22.0, vec!["value".into()])
        .unwrap();
    assert!(revision > first.revision);
    assert_eq!(collections.cache_rows(node), 0);
    assert_eq!(
        collections
            .provide(&first, vec![vec![json!(1)]; first.count as usize])
            .unwrap_err()
            .code,
        "stale_data"
    );
}

#[test]
fn row_cache_eviction_and_tree_snapshot_are_capped_and_preserve_selection() {
    let collections = Collections::new();
    let node = Handle(11);
    collections
        .create(node, 10_000, 18.0, vec!["value".into()])
        .unwrap();
    for batch in 0..5 {
        let start = batch * MAX_REQUEST_ROWS as u64;
        let request = collections
            .request_visible(node, start, MAX_REQUEST_ROWS, format!("r{batch}"))
            .unwrap()
            .unwrap();
        collections
            .provide(
                &request,
                (0..request.count)
                    .map(|i| vec![json!(format!("row-{}", request.start + i))])
                    .collect(),
            )
            .unwrap();
    }
    assert!(collections.cache_rows(node) <= MAX_CACHE_ROWS_PER_NODE);
    assert!(collections.cache_bytes(node) <= 8 * 1024 * 1024);
    assert!(collections.total_bytes() <= 64 * 1024 * 1024);

    let mut tree = TreeSnapshot::default();
    tree.replace(vec![TreeItem {
        key: "a".into(),
        label: "A".into(),
        children: vec![TreeItem {
            key: "b".into(),
            label: "B".into(),
            children: vec![],
        }],
    }])
    .unwrap();
    tree.select(Some("b".into())).unwrap();
    tree.replace(vec![TreeItem {
        key: "b".into(),
        label: "B renamed".into(),
        children: vec![],
    }])
    .unwrap();
    assert_eq!(tree.selected(), Some("b"));
    let too_deep = nested_tree(65);
    assert_eq!(
        tree.replace(vec![too_deep]).unwrap_err().code,
        "depth_exceeded"
    );
    assert_eq!(tree.selected(), Some("b"));
}

fn nested_tree(depth: usize) -> TreeItem {
    let mut item = TreeItem {
        key: format!("{depth}"),
        label: String::new(),
        children: vec![],
    };
    for index in (1..depth).rev() {
        item = TreeItem {
            key: index.to_string(),
            label: String::new(),
            children: vec![item],
        };
    }
    item
}
