use grafikr_ffi::{
    INVALID_ARGUMENT, INVALID_SESSION, WRONG_THREAD, grafikr_abi_version, grafikr_request_stop,
    grafikr_run, grafikr_session_create, grafikr_session_destroy, grafikr_submit,
};

#[test]
fn exported_surface_checks_arguments_and_thread_ownership() {
    assert_eq!(grafikr_abi_version(), 0x0001_0000);
    // SAFETY: null intentionally checks the ABI boundary.
    assert_eq!(
        unsafe { grafikr_session_create(std::ptr::null_mut()) },
        INVALID_ARGUMENT
    );
    let mut created = 0;
    // SAFETY: created is a valid output pointer.
    assert_eq!(unsafe { grafikr_session_create(&mut created) }, 0);
    assert_eq!(
        unsafe { grafikr_submit(created, std::ptr::null(), 1) },
        INVALID_ARGUMENT
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let creator = std::thread::spawn(move || {
        let mut id = 0;
        // SAFETY: id points to valid writable storage.
        assert_eq!(unsafe { grafikr_session_create(&mut id) }, 0);
        tx.send(id).unwrap();
        // Ensure the session remains registered while another thread calls run.
        std::thread::sleep(std::time::Duration::from_millis(100));
        id
    });
    let id = rx.recv().unwrap();
    assert_eq!(grafikr_run(id), WRONG_THREAD);
    let _ = creator.join().unwrap();
    assert_eq!(grafikr_session_destroy(id), 0);
    assert_eq!(grafikr_session_destroy(id), INVALID_SESSION);
    assert_eq!(grafikr_session_destroy(created), 0);
}

#[test]
fn stop_before_run_does_not_start_the_event_loop() {
    let mut id = 0;
    // SAFETY: id is writable storage.
    assert_eq!(unsafe { grafikr_session_create(&mut id) }, 0);
    assert_eq!(grafikr_request_stop(id), 0);
    assert_eq!(grafikr_run(id), 0);
    assert_eq!(grafikr_session_destroy(id), 0);
}
