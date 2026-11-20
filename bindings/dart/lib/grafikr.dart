import 'dart:async';
import 'dart:convert';
import 'dart:ffi' show Abi;
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';
import 'package:crypto/crypto.dart';

const maxFrameBytes = 8 * 1024 * 1024;
const maxOutstandingRequests = 256;
const _maxQueueMessages = 1024;
const _maxQueueBytes = 16 * 1024 * 1024;

class BridgeException implements Exception {
  BridgeException(this.code, this.message, [this.field]);
  final String code, message;
  final String? field;
  @override
  String toString() => '$code: $message';
}

class Runtime {
  static const _targets = {
    Abi.windowsX64: 'x86_64-pc-windows-msvc',
    Abi.windowsArm64: 'aarch64-pc-windows-msvc',
    Abi.linuxX64: 'x86_64-unknown-linux-gnu',
    Abi.linuxArm64: 'aarch64-unknown-linux-gnu',
    Abi.macosX64: 'x86_64-apple-darwin',
    Abi.macosArm64: 'aarch64-apple-darwin',
  };

  static Future<File> install(Directory bundle, Directory cache) async {
    final target = _targets[Abi.current()];
    if (target == null)
      throw BridgeException(
        'runtime_target',
        'Grafikr has no runtime for this platform',
      );
    final source = await _verify(bundle, target);
    final root = Directory(
      '${cache.absolute.path}${Platform.pathSeparator}$target${Platform.pathSeparator}${source.manifest['engine_version']}',
    );
    if (await FileSystemEntity.type(root.path, followLinks: false) !=
        FileSystemEntityType.notFound) {
      final existing = await _verify(root, target);
      final installed = File(
        '${root.path}${Platform.pathSeparator}${existing.host.replaceAll('/', Platform.pathSeparator)}',
      );
      await _makeExecutable(installed);
      return installed;
    }
    await root.parent.create(recursive: true);
    final stage = await Directory(
      root.parent.path,
    ).createTemp('.grafikr-runtime-');
    try {
      for (final item in source.files) {
        final relative = item['path'] as String;
        final destination = File(
          '${stage.path}${Platform.pathSeparator}${relative.replaceAll('/', Platform.pathSeparator)}',
        );
        await destination.parent.create(recursive: true);
        await File(
          '${bundle.absolute.path}${Platform.pathSeparator}${relative.replaceAll('/', Platform.pathSeparator)}',
        ).copy(destination.path);
      }
      await File(
        '${bundle.absolute.path}${Platform.pathSeparator}manifest.json',
      ).copy('${stage.path}${Platform.pathSeparator}manifest.json');
      await _verify(stage, target);
      await _makeExecutable(
        File(
          '${stage.path}${Platform.pathSeparator}${source.host.replaceAll('/', Platform.pathSeparator)}',
        ),
      );
      try {
        await stage.rename(root.path);
      } on FileSystemException {
        if (await FileSystemEntity.type(root.path, followLinks: false) ==
            FileSystemEntityType.directory) {
          final existing = await _verify(root, target);
          await stage.delete(recursive: true);
          final installed = File(
            '${root.path}${Platform.pathSeparator}${existing.host.replaceAll('/', Platform.pathSeparator)}',
          );
          await _makeExecutable(installed);
          return installed;
        }
        rethrow;
      }
      return File(
        '${root.path}${Platform.pathSeparator}${source.host.replaceAll('/', Platform.pathSeparator)}',
      );
    } catch (_) {
      if (await stage.exists()) await stage.delete(recursive: true);
      rethrow;
    }
  }

  static Future<void> _makeExecutable(File host) async {
    if (Platform.isWindows) return;
    final result = await Process.run('chmod', [
      'u+x',
      host.path,
    ], runInShell: false);
    if (result.exitCode != 0)
      throw BridgeException(
        'runtime_permissions',
        'Could not make runtime host executable',
      );
  }

  static Future<
    ({
      Map<String, Object?> manifest,
      List<Map<String, Object?>> files,
      String host,
    })
  >
  _verify(Directory root, String target) async {
    if (await FileSystemEntity.type(root.path, followLinks: false) !=
        FileSystemEntityType.directory) {
      throw BridgeException(
        'runtime_path',
        'Grafikr runtime directory is missing or unsafe',
      );
    }
    final manifestFile = File(
      '${root.path}${Platform.pathSeparator}manifest.json',
    );
    if (await FileSystemEntity.type(manifestFile.path, followLinks: false) !=
            FileSystemEntityType.file ||
        await manifestFile.length() > 1024 * 1024) {
      throw BridgeException(
        'runtime_manifest',
        'Grafikr runtime manifest is missing or unsafe',
      );
    }
    final decoded = jsonDecode(await manifestFile.readAsString());
    if (decoded is! Map<String, Object?> ||
        decoded['format_version'] != 1 ||
        decoded['target'] != target ||
        decoded['development_fixture'] != false ||
        decoded['engine_version'] is! String ||
        !(decoded['engine_version'] as String).contains(
          RegExp(r'^\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?$'),
        )) {
      throw BridgeException(
        'runtime_target',
        'Grafikr runtime target or release metadata is invalid',
      );
    }
    final entries = decoded['files'];
    if (entries is! List || entries.isEmpty)
      throw BridgeException(
        'runtime_manifest',
        'Grafikr runtime manifest has no files',
      );
    final files = <Map<String, Object?>>[];
    String? host;
    for (final entry in entries) {
      if (entry is! Map<String, Object?>)
        throw BridgeException(
          'runtime_manifest',
          'Grafikr runtime manifest entry is invalid',
        );
      final path = entry['path'];
      if (path is! String ||
          path.isEmpty ||
          path.contains('\\') ||
          path.startsWith('/') ||
          path
              .split('/')
              .any(
                (part) =>
                    part.isEmpty ||
                    part == '.' ||
                    part == '..' ||
                    part.contains(':'),
              )) {
        throw BridgeException(
          'runtime_path',
          'Grafikr runtime manifest contains an unsafe path',
        );
      }
      var parent = root.absolute.path;
      for (final part in path.split('/').take(path.split('/').length - 1)) {
        parent = '$parent${Platform.pathSeparator}$part';
        if (await FileSystemEntity.type(parent, followLinks: false) !=
            FileSystemEntityType.directory)
          throw BridgeException(
            'runtime_path',
            'Grafikr runtime path contains an unsafe parent',
          );
      }
      final file = File(
        '${root.absolute.path}${Platform.pathSeparator}${path.replaceAll('/', Platform.pathSeparator)}',
      );
      if (await FileSystemEntity.type(file.path, followLinks: false) !=
              FileSystemEntityType.file ||
          entry['size'] is! int ||
          await file.length() != entry['size'] ||
          entry['sha256'] is! String ||
          (await sha256.bind(file.openRead()).first).toString() !=
              entry['sha256']) {
        throw BridgeException(
          'runtime_checksum',
          'Grafikr runtime file failed verification: $path',
        );
      }
      if (entry['role'] == 'host') {
        if (host != null)
          throw BridgeException(
            'runtime_manifest',
            'Grafikr runtime manifest has multiple host executables',
          );
        host = path;
      }
      files.add(entry);
    }
    if (host == null)
      throw BridgeException(
        'runtime_manifest',
        'Grafikr runtime manifest has no host executable',
      );
    return (manifest: decoded, files: files, host: host);
  }
}

class _EventEnvelope {
  const _EventEnvelope(this.value, this.bytes);
  final Map<String, Object?> value;
  final int bytes;
}

void _readerEntry(List<Object?> args) {
  unawaited(_readerMain(args[0] as SendPort, args[1] as String));
}

Future<void> _readerMain(SendPort parent, String host) async {
  final commands = ReceivePort();
  late final Process process;
  try {
    process = await Process.start(host, const [], runInShell: false);
  } catch (error) {
    parent.send(['failure', 'connection_closed', '$error']);
    commands.close();
    return;
  }

  var stopped = false;
  var closing = false;
  var exitSent = false;
  var eventCount = 0;
  var eventBytes = 0;
  var outputCount = 0;
  var outputBytes = 0;
  Future<void>? writeFlush;
  final buffer = <int>[];
  final pendingWrites = <({Uint8List frame, int commandBytes})>[];
  final exitCode = process.exitCode;

  void fail(BridgeException error) {
    if (stopped) return;
    stopped = true;
    parent.send(['failure', error.code, error.message]);
    process.kill();
  }

  void reportExit(int code) {
    if (!exitSent) {
      exitSent = true;
      parent.send(['exit', code]);
    }
    commands.close();
  }

  void returnCommandCredit(int bytes) => parent.send(['command-credit', bytes]);

  Future<void> drainWrites() async {
    try {
      await Future<void>.delayed(Duration.zero);
      while (pendingWrites.isNotEmpty) {
        final writes = List.of(pendingWrites);
        pendingWrites.clear();
        for (final write in writes) {
          process.stdin.add(write.frame);
        }
        await process.stdin.flush();
        for (final write in writes) {
          returnCommandCredit(write.commandBytes);
        }
      }
    } catch (error) {
      fail(BridgeException('connection_closed', 'Host input failed: $error'));
      rethrow;
    } finally {
      writeFlush = null;
    }
  }

  Future<void> flushWrites() {
    return writeFlush ??= drainWrites();
  }

  void sendOutput(List<Object?> message, int bytes) {
    if (outputCount >= _maxQueueMessages ||
        outputBytes + bytes > _maxQueueBytes) {
      fail(BridgeException('queue_full', 'Host output queue quota reached'));
      return;
    }
    outputCount++;
    outputBytes += bytes;
    parent.send(message);
  }

  void consume(List<int> chunk) {
    if (stopped) return;
    buffer.addAll(chunk);
    while (!stopped) {
      final lf = buffer.indexOf(10);
      if (lf < 0) {
        if (buffer.length > maxFrameBytes) {
          fail(BridgeException('frame_too_large', 'Host frame exceeds 8 MiB'));
        }
        return;
      }
      final frameBytes = lf + 1;
      if (frameBytes > maxFrameBytes) {
        fail(BridgeException('frame_too_large', 'Host frame exceeds 8 MiB'));
        return;
      }
      try {
        final line = utf8.decode(buffer.sublist(0, lf), allowMalformed: false);
        buffer.removeRange(0, frameBytes);
        final decoded = jsonDecode(line);
        if (decoded is! Map<String, Object?>)
          throw const FormatException('Host frame must be a JSON object');
        final kind = decoded['kind'];
        if (kind == 'event') {
          if (eventCount >= _maxQueueMessages ||
              eventBytes + frameBytes > _maxQueueBytes) {
            fail(
              BridgeException('queue_full', 'Host event queue quota reached'),
            );
            return;
          }
          eventCount++;
          eventBytes += frameBytes;
          sendOutput(['event', decoded, frameBytes], frameBytes);
        } else if (kind == 'response') {
          sendOutput(['response', decoded, frameBytes], frameBytes);
        } else {
          fail(BridgeException('invalid_frame', 'Unknown host message kind'));
        }
      } catch (error) {
        fail(BridgeException('invalid_frame', 'Invalid host JSON: $error'));
      }
    }
  }

  void handleCommand(Object? raw) {
    if (raw is! List<Object?> || raw.isEmpty || raw[0] is! String) {
      fail(BridgeException('invalid_frame', 'Invalid reader command'));
      return;
    }
    final kind = raw[0] as String;
    if (kind == 'write') {
      final bytes = raw[1] as int;
      final commandBytes = raw[3] as int;
      if (closing || stopped) {
        returnCommandCredit(commandBytes);
        return;
      }
      try {
        final frame = (raw[2] as TransferableTypedData)
            .materialize()
            .asUint8List();
        if (frame.length != bytes || frame.length > maxFrameBytes) {
          fail(
            BridgeException('invalid_frame', 'Invalid request frame transfer'),
          );
          return;
        }
        pendingWrites.add((frame: frame, commandBytes: commandBytes));
        unawaited(flushWrites().catchError((_) {}));
      } catch (error) {
        fail(BridgeException('connection_closed', 'Host input failed: $error'));
      }
    } else if (kind == 'output-credit') {
      final bytes = raw[1] as int;
      final isEvent = raw[2] as bool;
      if (outputCount <= 0 ||
          outputBytes < bytes ||
          (isEvent && (eventCount <= 0 || eventBytes < bytes))) {
        fail(BridgeException('invalid_frame', 'Invalid output credit'));
        return;
      }
      outputCount--;
      outputBytes -= bytes;
      if (isEvent) {
        eventCount--;
        eventBytes -= bytes;
      }
      returnCommandCredit(raw[3] as int);
    } else if (kind == 'close') {
      if (closing) return;
      closing = true;
      final timeoutMilliseconds = raw[1] as int;
      final commandBytes = raw[2] as int;
      returnCommandCredit(commandBytes);
      try {
        unawaited(process.stdin.close().catchError((_) {}));
      } catch (_) {}
      unawaited(() async {
        try {
          await exitCode.timeout(Duration(milliseconds: timeoutMilliseconds));
        } on TimeoutException {
          process.kill();
        }
      }());
    } else {
      fail(BridgeException('invalid_frame', 'Unknown reader command'));
    }
  }

  exitCode.then<void>(
    reportExit,
    onError: (Object error, StackTrace stack) =>
        fail(BridgeException('connection_closed', '$error')),
  );
  process.stdout.listen(
    consume,
    onError: (Object error) =>
        fail(BridgeException('connection_closed', '$error')),
    onDone: () {
      if (buffer.isNotEmpty)
        fail(
          BridgeException(
            'invalid_frame',
            'Host exited before LF frame terminator',
          ),
        );
    },
  );
  process.stderr.listen((_) {});
  commands.listen(handleCommand);
  parent.send(['ready', commands.sendPort]);
}

class App {
  App._(this._messages, this.onError) {
    _messages.listen(_handleReaderMessage);
    _events.stream.listen(_enqueueDispatch);
  }

  final ReceivePort _messages;
  final void Function(Object, StackTrace)? onError;
  final Completer<SendPort> _ready = Completer<SendPort>();
  final Completer<void> _exited = Completer<void>();
  Isolate? _reader;
  SendPort? _commands;
  int _nextId = 0;
  int _commandCount = 0;
  int _commandBytes = 0;
  final Map<String, Completer<Object?>> _pending = {};
  final Map<String, List<FutureOr<void> Function(Map<String, Object?>)>>
  _handlers = {};
  final StreamController<_EventEnvelope> _events = StreamController();
  Future<void> _dispatchTail = Future<void>.value();
  bool _closed = false;

  static Future<App> start(
    String absoluteHostPath, {
    void Function(Object, StackTrace)? onError,
  }) async {
    final app = App._(ReceivePort(), onError);
    try {
      app._reader = await Isolate.spawn<List<Object?>>(
        _readerEntry,
        [app._messages.sendPort, absoluteHostPath],
        onExit: app._messages.sendPort,
        onError: app._messages.sendPort,
        debugName: 'grafikr-reader',
      );
      app._commands = await app._ready.future.timeout(
        const Duration(seconds: 10),
      );
      final hello =
          await app.request('hello', {
                'protocol_major': 1,
                'protocol_minor': 0,
                'sdk_version': '1.1.0',
              })
              as Map;
      if ((hello['protocol'] as Map?)?['major'] != 1) {
        throw BridgeException('version_mismatch', 'Protocol 1 required');
      }
      return app;
    } catch (_) {
      await app.close();
      rethrow;
    }
  }

  Future<Object?> request(
    String op,
    Map<String, Object?> args, {
    Duration timeout = const Duration(seconds: 30),
  }) async {
    if (_closed) throw BridgeException('connection_closed', 'App is closed');
    if (_pending.length >= maxOutstandingRequests)
      throw BridgeException(
        'queue_full',
        'Outstanding request count quota reached',
      );
    final id = (++_nextId).toString();
    late final String encoded;
    try {
      encoded = jsonEncode({'id': id, 'op': op, 'args': args});
    } on JsonUnsupportedObjectError {
      throw BridgeException(
        'invalid_value',
        'Request contains a value that cannot be encoded as JSON',
      );
    }
    final frame = utf8.encode('$encoded\n');
    if (frame.length > maxFrameBytes)
      throw BridgeException('frame_too_large', 'Request exceeds 8 MiB');
    final completer = Completer<Object?>();
    _pending[id] = completer;
    final response = completer.future.timeout(
      timeout,
      onTimeout: () {
        _pending.remove(id);
        throw BridgeException('timeout', 'Request timed out');
      },
    );
    try {
      _sendCommand('write', [
        frame.length,
        TransferableTypedData.fromList([Uint8List.fromList(frame)]),
      ], frame.length + 32);
    } catch (error) {
      _pending.remove(id);
      rethrow;
    }
    return response;
  }

  void _sendCommand(String kind, List<Object?> args, int bytes) {
    final commands = _commands;
    if (commands == null || (_closed && kind != 'close')) {
      throw BridgeException('connection_closed', 'Reader isolate is closed');
    }
    if (_commandCount >= _maxQueueMessages ||
        _commandBytes + bytes > _maxQueueBytes) {
      throw BridgeException('queue_full', 'Reader command queue quota reached');
    }
    _commandCount++;
    _commandBytes += bytes;
    try {
      commands.send([kind, ...args, bytes]);
    } catch (error) {
      _commandCount--;
      _commandBytes -= bytes;
      throw BridgeException(
        'connection_closed',
        'Reader command failed: $error',
      );
    }
  }

  Future<Subscription> on(
    String node,
    String event,
    FutureOr<void> Function(Map<String, Object?>) callback,
  ) async {
    final key = '$node\u0000$event';
    _handlers.putIfAbsent(key, () => []).add(callback);
    late final Map result;
    try {
      result =
          await request('event.subscribe', {'node': node, 'event': event})
              as Map;
    } catch (_) {
      _handlers[key]?.remove(callback);
      rethrow;
    }
    return Subscription(() async {
      _handlers[key]?.remove(callback);
      await request('event.unsubscribe', {
        'subscription': result['subscription'] as String,
      });
    });
  }

  Future<Window> createWindow(String title, double width, double height) async {
    final result =
        await request('window.create', {
              'title': title,
              'width': width,
              'height': height,
            })
            as Map;
    return Window(this, result['window'] as String, result['root'] as String);
  }

  Future<Node> createNode(
    Window window,
    String widget,
    Map<String, Object?> properties,
  ) async {
    final result =
        await request('node.create', {
              'window': window.handle,
              'widget': widget,
              'properties': properties,
            })
            as Map;
    return Node(this, result['node'] as String);
  }

  Future<State> createState(Map<String, Object?> value) async {
    final result = await request('state.create', {'value': value}) as Map;
    return State(this, result['state'] as String);
  }

  Future<CollectionProvider> createCollectionProvider(
    Node node,
    CollectionMetadata metadata,
  ) => CollectionProvider.create(node, metadata);

  void _handleReaderMessage(Object? raw) {
    if (raw == null) {
      if (!_ready.isCompleted)
        _ready.completeError(
          BridgeException(
            'connection_closed',
            'Reader isolate exited during startup',
          ),
        );
      if (!_exited.isCompleted) _exited.complete();
      _fail(BridgeException('connection_closed', 'Reader isolate exited'));
      return;
    }
    if (raw is List<Object?> &&
        raw.length == 2 &&
        raw.every((item) => item is String)) {
      final error = BridgeException('connection_closed', raw.first as String);
      if (!_ready.isCompleted) _ready.completeError(error);
      _fail(error);
      return;
    }
    if (raw is! List<Object?> || raw.isEmpty || raw[0] is! String) {
      _fail(BridgeException('invalid_frame', 'Invalid reader message'));
      return;
    }
    final kind = raw[0] as String;
    if (kind == 'ready') {
      final commands = raw[1];
      if (commands is! SendPort || _ready.isCompleted) {
        _fail(
          BridgeException('invalid_frame', 'Invalid reader startup message'),
        );
        return;
      }
      _commands = commands;
      _ready.complete(commands);
    } else if (kind == 'command-credit') {
      final bytes = raw[1];
      if (bytes is! int || _commandCount <= 0 || _commandBytes < bytes) {
        _fail(
          BridgeException('invalid_frame', 'Invalid reader command credit'),
        );
        return;
      }
      _commandCount--;
      _commandBytes -= bytes;
    } else if (kind == 'response') {
      final message = raw[1];
      final bytes = raw[2];
      if (message is! Map<String, Object?> || bytes is! int) {
        _fail(BridgeException('invalid_frame', 'Invalid host response'));
        return;
      }
      try {
        final id = message['id'];
        final ok = message['ok'];
        if (id is! String || ok is! bool)
          throw const FormatException('Response fields are invalid');
        final pending = _pending.remove(id);
        if (pending != null) {
          if (ok) {
            pending.complete(message['result']);
          } else {
            final error = message['error'];
            if (error is! Map)
              throw const FormatException('Response error is invalid');
            pending.completeError(
              BridgeException(
                error['code'] as String,
                error['message'] as String,
                error['field'] as String?,
              ),
            );
          }
        }
      } catch (error) {
        _fail(BridgeException('invalid_frame', '$error'));
      } finally {
        if (!_closed) _returnOutputCredit(bytes, false);
      }
    } else if (kind == 'event') {
      final message = raw[1];
      final bytes = raw[2];
      if (message is! Map<String, Object?> || bytes is! int) {
        _fail(BridgeException('invalid_frame', 'Invalid host event'));
        return;
      }
      if (_closed) {
        return;
      }
      _events.add(_EventEnvelope(message, bytes));
    } else if (kind == 'failure') {
      final code = raw.length > 1 ? raw[1] : null;
      final message = raw.length > 2 ? raw[2] : null;
      final error = BridgeException(
        code is String ? code : 'connection_closed',
        message is String ? message : 'Reader failed',
      );
      if (!_ready.isCompleted) _ready.completeError(error);
      _fail(error);
    } else if (kind == 'exit') {
      if (!_exited.isCompleted) _exited.complete();
      _fail(
        BridgeException('connection_closed', 'Host exited with code ${raw[1]}'),
      );
    } else {
      _fail(BridgeException('invalid_frame', 'Unknown reader message kind'));
    }
  }

  void _returnOutputCredit(int bytes, bool isEvent) {
    try {
      _sendCommand('output-credit', [bytes, isEvent], 16);
    } catch (error) {
      _fail(error);
    }
  }

  void _enqueueDispatch(_EventEnvelope envelope) {
    _dispatchTail = _dispatchTail
        .then((_) => _dispatch(envelope))
        .catchError((Object error, StackTrace stack) => _fail(error));
  }

  Future<void> _dispatch(_EventEnvelope envelope) async {
    if (_closed) return;
    final event = envelope.value;
    final key = '${event['source']}\u0000${event['event']}';
    for (final callback in List.of(_handlers[key] ?? const [])) {
      try {
        await callback(event);
      } catch (error, stack) {
        final handler = onError;
        if (handler == null) {
          Zone.current.handleUncaughtError(error, stack);
        } else {
          try {
            handler(error, stack);
          } catch (handlerError, handlerStack) {
            Zone.current.handleUncaughtError(handlerError, handlerStack);
          }
        }
      }
    }
    if (!_closed) _returnOutputCredit(envelope.bytes, true);
  }

  void _fail(Object error) {
    if (_closed && _pending.isEmpty) return;
    _closed = true;
    final failure = error is BridgeException
        ? error
        : BridgeException('connection_closed', '$error');
    for (final future in _pending.values) {
      if (!future.isCompleted) future.completeError(failure);
    }
    _pending.clear();
    _events.close();
  }

  Future<void> close() async {
    final elapsed = Stopwatch()..start();
    Duration remaining() {
      final duration = const Duration(seconds: 5) - elapsed.elapsed;
      return duration.isNegative ? Duration.zero : duration;
    }

    if (!_closed) {
      try {
        await request('shutdown', {}, timeout: remaining());
      } catch (_) {}
    }
    _fail(BridgeException('connection_closed', 'App is closed'));
    if (!_exited.isCompleted && _reader != null) {
      try {
        _sendCommand('close', [remaining().inMilliseconds], 16);
      } catch (_) {
        _reader!.kill(priority: Isolate.immediate);
      }
      try {
        await _exited.future.timeout(remaining());
      } on TimeoutException {
        try {
          await _exited.future.timeout(const Duration(milliseconds: 500));
        } on TimeoutException {
          _reader!.kill(priority: Isolate.immediate);
        }
      }
    }
    _messages.close();
  }
}

class Subscription {
  Subscription(this._dispose);
  final Future<void> Function() _dispose;
  bool _closed = false;
  Future<void> close() async {
    if (!_closed) {
      _closed = true;
      await _dispose();
    }
  }
}

class Window {
  Window(this.app, this.handle, this.root);
  final App app;
  final String handle, root;
  Future<Object?> close() => app.request('window.close', {'window': handle});
}

class Node {
  Node(this.app, this.handle);
  final App app;
  final String handle;
  Future<Object?> attach(String parent) =>
      app.request('node.attach', {'node': handle, 'parent': parent});
  Future<Object?> update(Map<String, Object?> properties) => app.request(
    'property.update',
    {'node': handle, 'properties': properties},
  );
  Future<Object?> destroy() => app.request('node.destroy', {'node': handle});
}

class State {
  State(this.app, this.handle);
  final App app;
  final String handle;
  Future<Object?> get() => app.request('state.get', {'state': handle});
  Future<Object?> set(Map<String, Object?> value) =>
      app.request('state.set', {'state': handle, 'value': value});
  Future<Object?> close() => app.request('state.destroy', {'state': handle});
}

class CollectionMetadata {
  const CollectionMetadata(this.rowCount, this.rowHeight, this.columns);
  final int rowCount;
  final double rowHeight;
  final List<String> columns;
  Map<String, Object?> toJson() {
    if (rowCount < 0 || !rowHeight.isFinite || rowHeight <= 0)
      throw BridgeException('invalid_value', 'Collection metadata is invalid');
    return {'row_count': rowCount, 'row_height': rowHeight, 'columns': columns};
  }
}

class CollectionRequest {
  const CollectionRequest(this.revision, this.request, this.start, this.count);
  final String revision, request;
  final int start, count;
  static CollectionRequest fromEvent(Map<String, Object?> event) {
    final payload = event['payload'];
    if (payload is! Map ||
        payload['revision'] is! String ||
        payload['request'] is! String ||
        payload['start'] is! int ||
        (payload['start'] as int) < 0 ||
        payload['count'] is! int ||
        (payload['count'] as int) < 0 ||
        (payload['count'] as int) > 512) {
      throw BridgeException(
        'invalid_frame',
        'Collection request fields are invalid',
      );
    }
    return CollectionRequest(
      payload['revision'] as String,
      payload['request'] as String,
      payload['start'] as int,
      payload['count'] as int,
    );
  }
}

class CollectionProvider {
  CollectionProvider._(this.node);
  final Node node;
  final List<CollectionRequest> _requests = [];
  Completer<CollectionRequest?>? _waiter;
  Subscription? _subscription;
  BridgeException? _failure;
  bool _closed = false, _disposed = false;

  static Future<CollectionProvider> create(
    Node node,
    CollectionMetadata metadata,
  ) async {
    final provider = CollectionProvider._(node);
    provider._subscription = await node.app.on(
      node.handle,
      'data.request',
      provider._receive,
    );
    try {
      await node.app.request('collection.create', {
        'node': node.handle,
        ...metadata.toJson(),
      });
      return provider;
    } catch (_) {
      await provider.close();
      rethrow;
    }
  }

  Future<CollectionRequest?> nextRequest({
    Duration timeout = const Duration(seconds: 30),
  }) async {
    if (_requests.isNotEmpty) return _requests.removeAt(0);
    final failure = _failure;
    if (failure != null) {
      _failure = null;
      throw failure;
    }
    if (_closed) return null;
    final waiter = Completer<CollectionRequest?>();
    _waiter = waiter;
    return waiter.future.timeout(
      timeout,
      onTimeout: () {
        if (identical(_waiter, waiter)) _waiter = null;
        return null;
      },
    );
  }

  Future<Object?> provide(CollectionRequest request, List<List<Object?>> rows) {
    if (_disposed)
      throw BridgeException(
        'connection_closed',
        'Collection provider is closed',
      );
    return node.app.request('data.provide', {
      'node': node.handle,
      'revision': request.revision,
      'request': request.request,
      'start': request.start,
      'rows': rows,
    });
  }

  Future<Object?> update(CollectionMetadata metadata) {
    if (_disposed)
      throw BridgeException(
        'connection_closed',
        'Collection provider is closed',
      );
    return node.app.request('collection.update', {
      'node': node.handle,
      'metadata': metadata.toJson(),
    });
  }

  Future<void> close() async {
    if (_disposed) return;
    _disposed = true;
    _closed = true;
    _requests.clear();
    final waiter = _waiter;
    _waiter = null;
    if (waiter != null && !waiter.isCompleted) waiter.complete(null);
    await _subscription?.close();
  }

  void _receive(Map<String, Object?> event) {
    if (_closed) return;
    CollectionRequest request;
    try {
      request = CollectionRequest.fromEvent(event);
    } on BridgeException catch (error) {
      _fail(error);
      return;
    }
    final waiter = _waiter;
    if (waiter != null) {
      _waiter = null;
      if (!waiter.isCompleted) waiter.complete(request);
    } else if (_requests.isEmpty) {
      _requests.add(request);
    } else {
      _fail(
        BridgeException(
          'queue_full',
          'Collection provider request queue is full',
        ),
      );
    }
  }

  void _fail(BridgeException error) {
    _failure = error;
    final waiter = _waiter;
    _waiter = null;
    if (waiter != null && !waiter.isCompleted && _requests.isEmpty)
      waiter.completeError(error);
  }
}
