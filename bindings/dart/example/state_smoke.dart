import 'dart:io';
import 'package:grafikr/grafikr.dart';

Future<void> main() async {
  final bundle = Platform.environment['GRAFIKR_BUNDLE'];
  final cache = Platform.environment['GRAFIKR_CACHE'];
  final directHost = Platform.environment['GRAFIKR_HOST'];
  final host = bundle != null && cache != null
      ? (await Runtime.install(Directory(bundle), Directory(cache))).path
      : directHost;
  if (host == null || host.isEmpty) {
    throw StateError('Set GRAFIKR_BUNDLE and GRAFIKR_CACHE, or GRAFIKR_HOST');
  }
  final app = await App.start(host);
  try {
    final state = await app.createState({
      'type': 'i64',
      'value': '-9223372036854775808',
    });
    final result = await state.get() as Map;
    if (result['value'] != '-9223372036854775808')
      throw StateError('signed i64 round trip failed');
    await state.close();
  } finally {
    await app.close();
  }
}
