# ADR 0005 — Surface and device recovery

**Status:** accepted (1.0)

## Context

The GPU can lose its device (driver reset, update, TDR) or a surface can become
outdated after a resize/monitor change. The application must not crash.

## Decision

- `Surface Lost`/`Outdated` immediately reconfigures with the last known
  configuration.
- `wgpu::Device::set_device_lost_callback` sends `UserEvent::DeviceLost` to the
  event loop. The `Destroyed` reason is ignored.
- The event loop recreates the instance, adapter, device, queue, surface,
  renderer, and atlas for every window. The UI tree, text engine, and application
  state stay alive.
- `CurrentSurfaceTexture::Validation` triggers the same full recovery.
- Recovery failures are logged and a redraw is requested again; the event loop
  keeps running.

## Consequences

- Glyph and image caches are lost during recovery; the next frame refills them.
- The callback runs on a wgpu thread and may only send an event; all resource
  creation happens on the event loop.
- There is no exponential backoff if recovery keeps failing.
