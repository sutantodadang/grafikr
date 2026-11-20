import 'dart:io';
import 'package:grafikr/grafikr.dart';

Future<void> main() async {
  final app = await App.start(Platform.environment['GRAFIKR_HOST']!);
  try {
    final window = await app.createWindow('Counter and form', 480, 320);
    final label = await app.createNode(window, 'Text', {'text': 'Count: 0'}); await label.attach(window.root);
    final button = await app.createNode(window, 'Button', {'label': 'Add'}); await button.attach(window.root);
    await app.on(button.handle, 'click', (_) { label.update({'text': 'Count: 1'}); });
    await ProcessSignal.sigint.watch().first;
  } finally { await app.close(); }
}
