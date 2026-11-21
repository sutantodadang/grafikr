import 'package:test/test.dart';
import 'package:grafikr/grafikr.dart';
import 'dart:async';
import 'dart:convert';
import 'dart:ffi' show Abi;
import 'dart:io';
import 'package:crypto/crypto.dart';

void main() {
  test('frame limit matches protocol 1', () {
    expect(maxFrameBytes, 8 * 1024 * 1024);
  });
  test('outstanding request limit matches SDK protocol contract', () {
    expect(maxOutstandingRequests, 256);
  });
  test('preserves signed 64-bit values through the reader isolate', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    try {
      for (final value in [-(1 << 63), (1 << 63) - 1]) {
        final scalar = {'type': 'i64', 'value': value.toString()};
        final state = await app.createState(scalar);
        expect(await state.get(), scalar);
        await state.close();
      }
    } finally {
      await app.close();
    }
  });
  test(
    'correlates out-of-order responses by request id',
    skip: Platform.environment.containsKey('GRAFIKR_FIXTURE_REVERSE_RESPONSES')
        ? false
        : 'requires the reverse-response fixture mode',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final app = await App.start(host);
      try {
        final first = await app.createState({
          'type': 'string',
          'value': 'first',
        });
        final second = await app.createState({
          'type': 'string',
          'value': 'second',
        });
        final values = await Future.wait([first.get(), second.get()]);
        expect(values, [
          {'type': 'string', 'value': 'first'},
          {'type': 'string', 'value': 'second'},
        ]);
        final marker = Platform.environment['GRAFIKR_FIXTURE_RESPONSE_MARKER'];
        expect(marker, isNotNull);
        expect(await File(marker!).readAsString(), 'second-before-first');
        await first.close();
        await second.close();
      } finally {
        await app.close();
      }
    },
  );
  test('round-trips a large UTF-8 frame with escaped newlines', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    try {
      final value = 'line one\nline two 雪 😀${'x' * (100 * 1024)}';
      final state = await app.createState({'type': 'string', 'value': value});
      expect(await state.get(), {'type': 'string', 'value': value});
      await state.close();
    } finally {
      await app.close();
    }
  });
  test('rejects non-finite values before sending a protocol frame', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    try {
      for (final value in [
        double.nan,
        double.infinity,
        double.negativeInfinity,
      ]) {
        await expectLater(
          app.request('state.create', {
            'value': {'type': 'f64', 'value': value},
          }),
          throwsA(
            isA<BridgeException>().having(
              (error) => error.code,
              'code',
              'invalid_value',
            ),
          ),
        );
      }
      final state = await app.createState({'type': 'i64', 'value': '7'});
      expect(await state.get(), {'type': 'i64', 'value': '7'});
      await state.close();
    } finally {
      await app.close();
    }
  });
  test('reader startup failure terminates the isolate once', () async {
    final executable = Platform.isWindows
        ? 'missing-fixture-host.exe'
        : 'missing-fixture-host';
    final host = File('../../target/debug/$executable').absolute.path;
    await expectLater(
      App.start(host),
      throwsA(
        isA<BridgeException>().having(
          (error) => error.code,
          'code',
          'connection_closed',
        ),
      ),
    );
  });
  test(
    'runtime installer verifies and atomically caches an unpacked bundle',
    () async {
      final temp = await Directory.systemTemp.createTemp('grafikr-runtime-');
      try {
        final bundle = Directory('${temp.path}${Platform.pathSeparator}bundle');
        final hostRelative = Platform.isWindows
            ? 'bin/grafikr-host.exe'
            : 'bin/grafikr-host';
        final host = File(
          '${bundle.path}${Platform.pathSeparator}${hostRelative.replaceAll('/', Platform.pathSeparator)}',
        );
        await host.parent.create(recursive: true);
        await host.writeAsString('fake host');
        final content = await host.readAsBytes();
        final manifest = {
          'format_version': 1,
          'engine_version': '1.1.0',
          'protocol': {'major': 1, 'minor': 0},
          'abi': {'major': 1, 'minor': 0},
          'target': _currentRuntimeTarget(),
          'min_os': 'test runtime',
          'min_glibc': Platform.isLinux ? '2.35' : null,
          'build_sha': List.filled(40, 'a').join(),
          'build_config_sha256': List.filled(64, 'b').join(),
          'source_dirty': false,
          'development_fixture': false,
          'system_requirements': <String>[],
          'limits': {
            'frame_bytes': 8388608,
            'json_depth': 64,
            'command_queue_messages': 1024,
            'command_queue_bytes': 16777216,
            'output_queue_messages': 1024,
            'output_queue_bytes': 16777216,
            'windows': 16,
            'nodes': 100000,
            'states': 100000,
            'subscriptions': 100000,
            'image_bytes': 67108864,
            'image_session_bytes': 268435456,
            'image_chunk_bytes': 1048576,
            'canvas_commands': 10000,
            'canvas_bytes': 8388608,
            'model_bytes': 134217728,
            'collection_rows_per_node': 2048,
            'collection_bytes_per_node': 8388608,
            'collection_bytes_session': 67108864,
            'collection_request_rows': 512,
            'tree_items': 50000,
            'tree_depth': 64,
            'host_shutdown_seconds': 5,
          },
          'files': [
            {
              'path': hostRelative,
              'size': content.length,
              'sha256': sha256.convert(content).toString(),
              'role': 'host',
            },
          ],
        };
        await File(
          '${bundle.path}${Platform.pathSeparator}manifest.json',
        ).writeAsString(jsonEncode(manifest));

        final cache = Directory('${temp.path}${Platform.pathSeparator}cache');
        final installed = await Runtime.install(bundle, cache);
        expect(await installed.readAsString(), 'fake host');
        expect((await Runtime.install(bundle, cache)).path, installed.path);
        expect(installed.path, contains('1.1.0'));
      } finally {
        await temp.delete(recursive: true);
      }
    },
  );
  test(
    'runtime installer rejects unsafe paths and changed file hashes',
    () async {
      final temp = await Directory.systemTemp.createTemp(
        'grafikr-runtime-invalid-',
      );
      try {
        final bundle = Directory('${temp.path}${Platform.pathSeparator}bundle');
        final hostRelative = Platform.isWindows
            ? 'bin/grafikr-host.exe'
            : 'bin/grafikr-host';
        final host = File(
          '${bundle.path}${Platform.pathSeparator}${hostRelative.replaceAll('/', Platform.pathSeparator)}',
        );
        await host.parent.create(recursive: true);
        await host.writeAsString('fake host');
        final content = await host.readAsBytes();
        final manifest = {
          'format_version': 1,
          'engine_version': '1.1.0',
          'target': _currentRuntimeTarget(),
          'development_fixture': false,
          'files': [
            {
              'path': '../escape',
              'size': content.length,
              'sha256': sha256.convert(content).toString(),
              'role': 'host',
            },
          ],
        };
        final manifestFile = File(
          '${bundle.path}${Platform.pathSeparator}manifest.json',
        );
        await manifestFile.writeAsString(jsonEncode(manifest));
        await expectLater(
          Runtime.install(bundle, Directory('${temp.path}/unsafe-cache')),
          throwsA(isA<BridgeException>()),
        );

        manifest['files'] = [
          {
            'path': hostRelative,
            'size': content.length,
            'sha256': sha256.convert(content).toString(),
            'role': 'host',
          },
        ];
        manifest['engine_version'] = '../../outside-cache';
        await manifestFile.writeAsString(jsonEncode(manifest));
        await expectLater(
          Runtime.install(bundle, Directory('${temp.path}/version-cache')),
          throwsA(isA<BridgeException>()),
        );
        manifest['engine_version'] = '1.1.0';
        await manifestFile.writeAsString(jsonEncode(manifest));
        await host.writeAsString('tampered host');
        await expectLater(
          Runtime.install(bundle, Directory('${temp.path}/tampered-cache')),
          throwsA(isA<BridgeException>()),
        );
      } finally {
        await temp.delete(recursive: true);
      }
    },
  );
  test('rejects requests beyond the outstanding request limit', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    var closed = false;
    try {
      final requests = List.generate(
        maxOutstandingRequests + 1,
        (_) => app.request('state.get', {'state': '0'}),
      );
      final outcomes = requests.map((request) async {
        try {
          await request;
          return 'success';
        } on BridgeException catch (error) {
          return error.code;
        }
      }).toList();
      final first = await Future.any(
        outcomes,
      ).timeout(const Duration(seconds: 2));
      expect(first, 'queue_full');
      await app.close();
      closed = true;
      final codes = await Future.wait(outcomes);
      expect(codes.where((code) => code == 'queue_full'), hasLength(1));
      expect(
        codes.where((code) => code == 'connection_closed'),
        hasLength(maxOutstandingRequests),
      );
    } finally {
      if (!closed) await app.close();
    }
  });
  test(
    'shutdown timeout kills only the Grafikr child process',
    skip: Platform.environment.containsKey('GRAFIKR_FIXTURE_DELAY_EXIT_MS')
        ? false
        : 'requires the delayed-exit fixture mode',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final markerPath = Platform.environment['GRAFIKR_FIXTURE_EXIT_MARKER'];
      expect(markerPath, isNotNull);
      final marker = File(markerPath!);
      if (marker.existsSync()) marker.deleteSync();
      final app = await App.start(host);
      final sentinel = await Process.start(host, [
        '--sleep-ms',
        '10000',
      ], runInShell: false);
      try {
        final stopwatch = Stopwatch()..start();
        await app.close().timeout(const Duration(seconds: 7));
        expect(marker.existsSync(), isTrue);
        expect(stopwatch.elapsed, lessThan(const Duration(seconds: 6)));
        final sentinelStatus = await sentinel.exitCode.timeout(
          const Duration(milliseconds: 100),
          onTimeout: () => -1,
        );
        expect(sentinelStatus, -1);
      } finally {
        sentinel.kill();
        await sentinel.exitCode;
        await app.close();
        if (marker.existsSync()) marker.deleteSync();
      }
    },
  );
  test(
    'shutdown deadline includes an unresponsive host',
    skip:
        Platform.environment.containsKey('GRAFIKR_FIXTURE_PAUSE_AFTER_HELLO_MS')
        ? false
        : 'requires the paused-after-hello fixture mode',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final markerPath = Platform.environment['GRAFIKR_FIXTURE_PAUSE_MARKER'];
      expect(markerPath, isNotNull);
      final marker = File(markerPath!);
      if (marker.existsSync()) marker.deleteSync();
      final app = await App.start(host);
      try {
        final deadline = DateTime.now().add(const Duration(seconds: 1));
        while (!marker.existsSync() && DateTime.now().isBefore(deadline)) {
          await Future<void>.delayed(const Duration(milliseconds: 10));
        }
        expect(marker.existsSync(), isTrue);
        final stopwatch = Stopwatch()..start();
        await app.close().timeout(const Duration(seconds: 9));
        expect(stopwatch.elapsed, lessThan(const Duration(seconds: 6)));
      } finally {
        await app.close();
        if (marker.existsSync()) marker.deleteSync();
      }
    },
  );
  test(
    'bounds queued command bytes while the host is unresponsive',
    skip:
        Platform.environment.containsKey('GRAFIKR_FIXTURE_PAUSE_AFTER_HELLO_MS')
        ? false
        : 'requires the paused-after-hello fixture mode',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final markerPath = Platform.environment['GRAFIKR_FIXTURE_PAUSE_MARKER'];
      expect(markerPath, isNotNull);
      final marker = File(markerPath!);
      if (marker.existsSync()) marker.deleteSync();
      final app = await App.start(host);
      try {
        final deadline = DateTime.now().add(const Duration(seconds: 1));
        while (!marker.existsSync() && DateTime.now().isBefore(deadline)) {
          await Future<void>.delayed(const Duration(milliseconds: 10));
        }
        expect(marker.existsSync(), isTrue);

        final value = 'x' * (6 * 1024 * 1024);
        final first = app.request('state.create', {
          'value': {'type': 'string', 'value': value},
        });
        final second = app.request('state.create', {
          'value': {'type': 'string', 'value': value},
        });
        final outcomes = [first, second].map((request) async {
          try {
            await request;
            return 'success';
          } on BridgeException catch (error) {
            return error.code;
          }
        }).toList();
        await expectLater(
          app.request('state.create', {
            'value': {'type': 'string', 'value': value},
          }),
          throwsA(
            isA<BridgeException>().having(
              (error) => error.code,
              'code',
              'queue_full',
            ),
          ),
        );
        await app.close();
        expect(await Future.wait(outcomes), [
          'connection_closed',
          'connection_closed',
        ]);
      } finally {
        await app.close();
        if (marker.existsSync()) marker.deleteSync();
      }
    },
  );
  test(
    'reader isolate drains host output while an event callback blocks',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final marker = File('../../target/dart-reader-progress.marker').absolute;
      if (marker.existsSync()) marker.deleteSync();
      final app = await App.start(host);
      try {
        final window = await app.createWindow('reader progress', 320, 240);
        final field = await app.createNode(window, 'TextField', {'text': ''});
        await field.attach(window.root);
        final callbackFinished = Completer<void>();
        var callbackCheckedReaderProgress = false;
        final subscription = await app.on(field.handle, 'change', (_) {
          if (callbackFinished.isCompleted) return;
          final deadline = DateTime.now().add(const Duration(seconds: 10));
          while (DateTime.now().isBefore(deadline) && !marker.existsSync()) {
            sleep(const Duration(milliseconds: 10));
          }
          callbackCheckedReaderProgress = marker.existsSync();
          callbackFinished.complete();
        });
        await callbackFinished.future.timeout(const Duration(seconds: 12));
        expect(callbackCheckedReaderProgress, isTrue);
        await subscription.close();
      } finally {
        await app.close();
        if (marker.existsSync()) marker.deleteSync();
      }
    },
  );
  test(
    'reader isolate enforces output credits by message count and bytes',
    () async {
      await _expectReaderOverflow('ComboBox', 'select', {
        'options': ['one'],
      });
      await _expectReaderOverflow('Canvas', 'resize', {});
    },
  );
  test('delivers an event sent before the subscribe response', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    try {
      final window = await app.createWindow('fixture', 320, 240);
      final button = await app.createNode(window, 'Button', {'label': 'click'});
      await button.attach(window.root);
      final received = Completer<Map<String, Object?>>();
      final subscription = await app.on(
        button.handle,
        'click',
        received.complete,
      );
      final event = await received.future.timeout(const Duration(seconds: 2));
      expect(event['source'], button.handle);
      expect(event['event'], 'click');
      await subscription.close();
    } finally {
      await app.close();
    }
  });
  test('reports callback exceptions without stopping later requests', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final callbackError = Completer<Object>();
    final app = await App.start(
      host,
      onError: (error, _) {
        if (!callbackError.isCompleted) callbackError.complete(error);
      },
    );
    try {
      final window = await app.createWindow('callback error', 320, 240);
      final button = await app.createNode(window, 'Button', {'label': 'click'});
      await button.attach(window.root);
      final subscription = await app.on(button.handle, 'click', (_) async {
        await Future<void>.delayed(Duration.zero);
        throw StateError('fixture callback exception');
      });
      expect(
        await callbackError.future.timeout(const Duration(seconds: 2)),
        isA<StateError>(),
      );
      final state = await app.createState({'type': 'i64', 'value': '9'});
      expect(await state.get(), {'type': 'i64', 'value': '9'});
      await state.close();
      await subscription.close();
    } finally {
      await app.close();
    }
  });
  test(
    'an event callback can issue a nested request without blocking responses',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final app = await App.start(host);
      try {
        final state = await app.createState({'type': 'i64', 'value': '0'});
        final window = await app.createWindow('nested callback', 320, 240);
        final button = await app.createNode(window, 'Button', {
          'label': 'click',
        });
        await button.attach(window.root);
        final result = Completer<Object?>();
        final subscription = await app.on(button.handle, 'click', (_) async {
          try {
            await state.set({'type': 'i64', 'value': '7'});
            result.complete(await state.get());
          } catch (error, stack) {
            result.completeError(error, stack);
          }
        });
        expect(await result.future.timeout(const Duration(seconds: 2)), {
          'type': 'i64',
          'value': '7',
        });
        await subscription.close();
        await state.close();
      } finally {
        await app.close();
      }
    },
  );
  test('unsubscribe suppresses an event queued before its response', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    var callbackCount = 0;
    try {
      final window = await app.createWindow('unsubscribe race', 320, 240);
      final button = await app.createNode(window, 'Button', {'label': 'click'});
      await button.attach(window.root);
      final subscription = await app.on(
        button.handle,
        'click',
        (_) => callbackCount++,
      );
      final callbacksBeforeUnsubscribe = callbackCount;
      await subscription.close();
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(callbackCount, callbacksBeforeUnsubscribe);
    } finally {
      await app.close();
    }
  });
  test(
    'async event callbacks are dispatched sequentially under a burst',
    () async {
      const burstCount = 512;
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final app = await App.start(host);
      var active = 0;
      var maxActive = 0;
      final indices = <int>[];
      final completed = Completer<void>();
      try {
        final window = await app.createWindow('async callback burst', 320, 240);
        final field = await app.createNode(window, 'TextField', {'text': ''});
        await field.attach(window.root);
        final subscription = await app.on(field.handle, 'change', (
          event,
        ) async {
          active++;
          if (active > maxActive) maxActive = active;
          await Future<void>.delayed(Duration.zero);
          final payload = event['payload'] as Map;
          indices.add(payload['index'] as int);
          active--;
          if (indices.length == burstCount && !completed.isCompleted) {
            completed.complete();
          }
        });
        await completed.future.timeout(const Duration(seconds: 8));
        expect(maxActive, 1);
        expect(indices, List<int>.generate(burstCount, (index) => index));
        await subscription.close();
      } finally {
        await app.close();
      }
    },
  );
  test('collection provider handles an early visible-range request', () async {
    final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
    final host =
        Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
        File('../../target/debug/$executable').absolute.path;
    final app = await App.start(host);
    try {
      final window = await app.createWindow('collection fixture', 320, 240);
      final list = await app.createNode(window, 'VirtualList', {
        'row_count': 1,
        'row_height': 24,
        'height': 120,
      });
      await list.attach(window.root);
      final provider = await app.createCollectionProvider(
        list,
        const CollectionMetadata(1, 24, ['label']),
      );
      final request = await provider.nextRequest(
        timeout: const Duration(seconds: 2),
      );
      expect(request?.revision, '1');
      expect(request?.start, 0);
      await provider.provide(request!, [
        ['row'],
      ]);
      await provider.update(const CollectionMetadata(1, 24, ['label']));
      await provider.close();
      await window.close();
    } finally {
      await app.close();
    }
  });
  test(
    'creates every shared widget and receives its declared events',
    skip: Platform.environment.containsKey('GRAFIKR_FIXTURE_WIDGET_INVENTORY')
        ? false
        : 'requires the isolated widget inventory fixture mode',
    () async {
      final executable = Platform.isWindows
          ? 'fixture-host.exe'
          : 'fixture-host';
      final host =
          Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
          File('../../target/debug/$executable').absolute.path;
      final inventory =
          jsonDecode(
                File(
                  '../../bindings/protocol/v1/widgets.json',
                ).readAsStringSync(),
              )
              as Map<String, Object?>;
      final widgets = inventory['widgets'] as Map<String, Object?>;
      final app = await App.start(host);
      try {
        final window = await app.createWindow('widget inventory', 640, 480);
        final state = await app.createState({'type': 'i64', 'value': '1'});
        final upload =
            (await app.request('image.begin', {
                      'width': 1,
                      'height': 1,
                      'byte_length': 4,
                    })
                    as Map)['upload']
                as String;
        await app.request('image.chunk', {
          'upload': upload,
          'offset': 0,
          'data': base64.encode([0, 0, 0, 255]),
        });
        final image =
            (await app.request('image.finalize', {'upload': upload})
                    as Map)['image']
                as String;

        for (final entry in widgets.entries) {
          final kind = entry.key;
          final spec = entry.value as Map<String, Object?>;
          final propertyTypes = spec['properties'] as Map<String, Object?>;
          final properties = <String, Object?>{
            for (final property in propertyTypes.entries)
              property.key: _sampleWidgetValue(
                kind,
                property.key,
                property.value as String,
                state.handle,
                image,
              ),
          };
          late final Node node;
          try {
            node = await app.createNode(window, kind, properties);
          } catch (error, stack) {
            Error.throwWithStackTrace(
              StateError('$kind create failed: $error'),
              stack,
            );
          }
          await node.attach(window.root);

          for (final event
              in (spec['events'] as List? ?? const []).cast<String>()) {
            final received = Completer<Map<String, Object?>>();
            final subscription = await app.on(node.handle, event, (message) {
              if (!received.isCompleted) received.complete(message);
            });
            final message = await received.future.timeout(
              const Duration(seconds: 2),
            );
            expect(message['source'], node.handle, reason: '$kind@$event');
            expect(message['event'], event, reason: '$kind@$event');
            await subscription.close();
          }
          await node.destroy();
        }

        await app.request('image.destroy', {'image': image});
        await state.close();
        await window.close();
      } finally {
        await app.close();
      }
    },
  );
}

Object _sampleWidgetValue(
  String kind,
  String property,
  String type,
  String state,
  String image,
) {
  if (kind == 'Radio' && property == 'group') return state;
  if (kind == 'TreeView' && property == 'selected') return 'root';
  if (kind == 'Slider') {
    return switch (property) {
      'min' => 0.0,
      'max' => 10.0,
      'width' => 120.0,
      _ => 1.0,
    };
  }
  if (kind == 'Spinner' && property == 'size') return 16.0;
  if ((kind == 'VirtualList' || kind == 'DataGrid') &&
      property == 'row_count') {
    return 1;
  }
  if ((kind == 'VirtualList' || kind == 'DataGrid') &&
      property == 'row_height') {
    return 24.0;
  }
  return switch (type) {
    'number' || 'f32' => 1.0,
    'u16' => 1,
    'u64' => 0,
    'string' => 'sample',
    'color' => '#336699',
    'transform' => {'x': 0.0, 'y': 0.0, 'scale': 1.0},
    'boolean' => false,
    'primary|secondary|icon' => 'primary',
    'state' => state,
    'GridColumn[]' => [
      {'title': 'Column', 'width': 120.0},
    ],
    'string[]' => ['One', 'Two'],
    'TreeNode[]' => [
      {'key': 'root', 'label': 'Root', 'children': <Object?>[]},
    ],
    'CanvasCommand[]' => [
      {
        'kind': 'rect',
        'rect': {'x': 0.0, 'y': 0.0, 'width': 1.0, 'height': 1.0},
        'color': '#336699',
        'radius': 0.0,
      },
    ],
    'ImageHandle' => image,
    _ => throw StateError(
      'No Dart conformance value for $kind.$property: $type',
    ),
  };
}

Future<void> _expectReaderOverflow(
  String widget,
  String event,
  Map<String, Object?> properties,
) async {
  final executable = Platform.isWindows ? 'fixture-host.exe' : 'fixture-host';
  final host =
      Platform.environment['GRAFIKR_FIXTURE_HOST'] ??
      File('../../target/debug/$executable').absolute.path;
  final marker = File('../../target/dart-reader-progress.marker').absolute;
  if (marker.existsSync()) marker.deleteSync();
  final app = await App.start(host);
  var callbackEntered = false;
  try {
    final window = await app.createWindow('reader overflow', 320, 240);
    final node = await app.createNode(window, widget, properties);
    await node.attach(window.root);
    await app.on(node.handle, event, (_) {
      if (callbackEntered) return;
      callbackEntered = true;
      final deadline = DateTime.now().add(const Duration(seconds: 5));
      while (DateTime.now().isBefore(deadline)) {}
    });
    final pending = app.request('state.get', {
      'state': '0',
    }, timeout: const Duration(seconds: 12));
    Object? result;
    try {
      await pending;
    } catch (error) {
      result = error;
    }
    expect(callbackEntered, isTrue);
    expect(
      result,
      isA<BridgeException>().having(
        (error) => error.code,
        'code',
        'queue_full',
      ),
    );
  } finally {
    await app.close();
  }
}

String _currentRuntimeTarget() => switch (Abi.current()) {
  Abi.windowsX64 => 'x86_64-pc-windows-msvc',
  Abi.windowsArm64 => 'aarch64-pc-windows-msvc',
  Abi.linuxX64 => 'x86_64-unknown-linux-gnu',
  Abi.linuxArm64 => 'aarch64-unknown-linux-gnu',
  Abi.macosX64 => 'x86_64-apple-darwin',
  Abi.macosArm64 => 'aarch64-apple-darwin',
  _ => 'unsupported-test-target',
};
