use grafikr_bridge::resources::Resources;

#[test]
fn image_upload_is_ordered_bounded_and_keeps_attached_arcs_alive() {
    let mut resources = Resources::new();
    assert_eq!(resources.begin(1, 1, 3).unwrap_err().code, "invalid_value");
    let upload = resources.begin(1, 1, 4).unwrap();
    assert_eq!(
        resources.append(upload, 1, &[1]).unwrap_err().code,
        "invalid_offset"
    );
    assert_eq!(
        resources.finalize(upload).unwrap_err().code,
        "incomplete_upload"
    );
    resources.append(upload, 0, &[1, 2]).unwrap();
    resources.append(upload, 2, &[3, 4]).unwrap();
    let image = resources.finalize(upload).unwrap();
    let attached = resources.image(image).unwrap();
    resources.release(image).unwrap();
    assert_eq!(resources.reserved_bytes(), 4);
    drop(attached);
    resources.begin(1, 1, 4).unwrap();
    assert_eq!(resources.reserved_bytes(), 4);
}
