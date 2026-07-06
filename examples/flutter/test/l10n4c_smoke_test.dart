import 'package:flutter_test/flutter_test.dart';
import 'package:l10n4x_example/l10n4c.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('L10n4c loads signed lpks and translates', () async {
    await L10n4c.init(fallbackLocale: 'en');

    expect(await L10n4c.loadLocaleFromAsset('es'), isTrue);
    expect(L10n4c.translate('es', 'common.welcome'), '¡Bienvenido!');
    expect(L10n4c.translate('en', 'common.welcome'), 'Welcome!');

    L10n4c.clear();
  });
}