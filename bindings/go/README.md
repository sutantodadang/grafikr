# Grafikr Go SDK

Go 1.26 module using only the standard library. Install a previously unpacked
bundle with `grafikr-runtime --bundle <directory> --cache <directory>`, or use
`grafikr.StartWithRuntime(bundle, cache)` to install and start it. `InstallRuntime`
validates the target, paths, sizes, and SHA-256 hashes and atomically fills a
versioned cache; it never downloads a runtime. `grafikr.Start(path)` remains
available when an absolute host path is already known. The SDK launches
the process with an argument array, bounds JSON Lines reads/writes, correlates
pending requests, drains stderr, and isolates panicking event callbacks.
`Close` requests shutdown and kills only its child after five seconds.

`App.CreateCanvas` creates a typed `Canvas` facade, and `Canvas.Update` sends
the protocol canvas command list without remounting the window. Canvas command
shapes follow `bindings/protocol/v1/README.md` and `schema.json`.

`App.UploadImage` accepts tightly packed RGBA bytes and streams protocol-sized
chunks; call `Image.Destroy` when the image is no longer owned. Protocol 1 has
no upload-abort operation, so a failed partial upload remains reserved until
the app session closes.

`App.CreateCollectionProvider` subscribes before publishing collection
metadata, so the first visible-range request is not lost. Read `Requests()` in
a worker, answer each revision-tagged range with `Provide`, call `Update` when
the collection changes, and call `Close` to unsubscribe and close the request
stream.
