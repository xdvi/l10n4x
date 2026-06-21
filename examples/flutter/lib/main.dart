import 'package:flutter/material.dart';

import 'l10n4c.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await L10n4c.init(fallbackLocale: 'en');
  runApp(const L10n4xExampleApp());
}

class L10n4xExampleApp extends StatelessWidget {
  const L10n4xExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'l10n4x Flutter Example',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.teal),
        useMaterial3: true,
      ),
      home: const HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  static const _locales = ['en', 'es'];
  static const _demoKey = 'common.welcome';

  String _locale = 'en';
  String _message = _demoKey;
  bool _loading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _refreshTranslation();
  }

  Future<void> _refreshTranslation() async {
    setState(() {
      _loading = true;
      _error = null;
    });

    try {
      final loaded = await L10n4c.loadLocaleFromAsset(_locale);
      if (!loaded) {
        throw StateError(
          'Could not load assets/locales/$_locale.pak — run l10n4x build and copy examples/dist/locales/*.pak',
        );
      }
      final text = L10n4c.translate(_locale, _demoKey);
      setState(() {
        _message = text;
        _loading = false;
      });
    } on Object catch (e) {
      setState(() {
        _error = e.toString();
        _message = _demoKey;
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('l10n4x + Flutter'),
      ),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Locale',
              style: Theme.of(context).textTheme.labelLarge,
            ),
            const SizedBox(height: 8),
            SegmentedButton<String>(
              segments: _locales
                  .map((l) => ButtonSegment(value: l, label: Text(l.toUpperCase())))
                  .toList(),
              selected: {_locale},
              onSelectionChanged: (selection) {
                setState(() => _locale = selection.first);
                _refreshTranslation();
              },
            ),
            const SizedBox(height: 32),
            Text(
              'Key: $_demoKey',
              style: Theme.of(context).textTheme.labelLarge,
            ),
            const SizedBox(height: 8),
            if (_loading)
              const Center(child: CircularProgressIndicator())
            else
              Text(
                _message,
                style: Theme.of(context).textTheme.headlineSmall,
              ),
            if (_error != null) ...[
              const SizedBox(height: 24),
              Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ],
            const Spacer(),
            Text(
              'Native lib: GitHub Releases → examples/lib/\n'
              'Verify key: --dart-define=L10N4X_VERIFY_PUBLIC_KEY=<hex>\n'
              'Paks: l10n4x build → copy to assets/locales/',
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
      ),
    );
  }
}