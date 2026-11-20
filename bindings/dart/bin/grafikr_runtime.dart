import 'dart:io';
import 'package:grafikr/grafikr.dart';

Future<void> main(List<String> arguments) async {
  if (arguments.length != 2) {
    stderr.writeln(
      'Usage: dart run bin/grafikr_runtime.dart <unpacked-bundle> <cache-directory>',
    );
    exitCode = 64;
    return;
  }
  try {
    stdout.writeln(
      (await Runtime.install(
        Directory(arguments[0]),
        Directory(arguments[1]),
      )).path,
    );
  } on BridgeException catch (error) {
    stderr.writeln(error);
    exitCode = 1;
  }
}
