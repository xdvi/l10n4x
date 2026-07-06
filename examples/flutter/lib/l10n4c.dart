import 'dart:ffi' as ffi;
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart' show rootBundle;

/// GitHub Releases URL for prebuilt `l10n4c` binaries.
const l10n4xReleasesUrl = 'https://github.com/xdvi/l10n4x/releases/latest';

const _verifyKeyFromDefine = String.fromEnvironment('L10N4X_VERIFY_PUBLIC_KEY');
const _encryptKeyFromDefine = String.fromEnvironment('L10N4X_ENCRYPT_KEY');

final class L10n4cParam extends ffi.Struct {
  external ffi.Pointer<Utf8> key;
  external ffi.Pointer<Utf8> value;
}

typedef _SetVerifyKeyNative = ffi.Int32 Function(
  ffi.Pointer<ffi.Uint8> key,
  ffi.Int keyLen,
);
typedef _SetVerifyKey = int Function(ffi.Pointer<ffi.Uint8> key, int keyLen);

typedef _SetDecryptKeyNative = ffi.Int32 Function(
  ffi.Pointer<ffi.Uint8> key,
  ffi.Int keyLen,
);
typedef _SetDecryptKey = int Function(ffi.Pointer<ffi.Uint8> key, int keyLen);

typedef _SetFallbackLocaleNative = ffi.Int32 Function(ffi.Pointer<Utf8> locale);
typedef _SetFallbackLocale = int Function(ffi.Pointer<Utf8> locale);

typedef _LoadLpkLocaleNative = ffi.Int32 Function(
  ffi.Pointer<Utf8> locale,
  ffi.Pointer<Utf8> filePath,
);
typedef _LoadLpkLocale = int Function(
  ffi.Pointer<Utf8> locale,
  ffi.Pointer<Utf8> filePath,
);

typedef _TranslateAllocNative = ffi.Pointer<Utf8> Function(
  ffi.Pointer<Utf8> locale,
  ffi.Uint64 keyHash,
);
typedef _TranslateAlloc = ffi.Pointer<Utf8> Function(
  ffi.Pointer<Utf8> locale,
  int keyHash,
);

typedef _TranslateWithParamsAllocNative = ffi.Pointer<Utf8> Function(
  ffi.Pointer<Utf8> locale,
  ffi.Uint64 keyHash,
  ffi.Pointer<L10n4cParam> params,
  ffi.Int paramCount,
);
typedef _TranslateWithParamsAlloc = ffi.Pointer<Utf8> Function(
  ffi.Pointer<Utf8> locale,
  int keyHash,
  ffi.Pointer<L10n4cParam> params,
  int paramCount,
);

typedef _FreeStringNative = ffi.Void Function(ffi.Pointer<Utf8> ptr);
typedef _FreeString = void Function(ffi.Pointer<Utf8> ptr);

typedef _ClearNative = ffi.Void Function();
typedef _Clear = void Function();

/// FNV-1a 64‑bit hash — must match `l10n4x_core::binary_format::fnv1a_64`.
///
/// Uses BigInt internally, then serialises to unsigned 64‑bit LE bytes
/// and reads back as signed int64 so that FFI receives the correct bits
/// regardless of whether the top bit is set.
int _fnv1a_64(String data) {
  final BigInt offset = BigInt.from(0xcbf29ce484222325);
  final BigInt prime = BigInt.from(0x100000001b3);
  final BigInt mask64 = BigInt.from(0xFFFFFFFFFFFFFFFF);
  var hash = offset;
  for (final b in data.codeUnits) {
    hash ^= BigInt.from(b);
    hash = (hash * prime) & mask64;
  }
  // Write the 64‑bit unsigned value in little‑endian, reinterpret as int64.
  final bytes = Uint8List(8);
  var v = hash;
  for (var i = 0; i < 8; i++) {
    bytes[i] = (v & BigInt.from(0xFF)).toInt();
    v >>= 8;
  }
  return ByteData.sublistView(bytes).getInt64(0, Endian.little);
}

/// Thin FFI wrapper around `l10n4c` for Flutter apps.
class L10n4c {
  L10n4c._();

  static const ok = 0;

  static late final ffi.DynamicLibrary _lib;
  static late final _SetVerifyKey _setVerifyKey;
  static late final _SetDecryptKey _setDecryptKey;
  static late final _SetFallbackLocale _setFallbackLocale;
  static late final _LoadLpkLocale _loadLpkLocale;
  static late final _TranslateAlloc _translateAlloc;
  static late final _TranslateWithParamsAlloc _translateWithParamsAlloc;
  static late final _FreeString _freeString;
  static late final _Clear _clear;

  static var _initialized = false;
  static final _loadedLocales = <String>{};
  static var _fallback = 'en';

  static Future<void> init({String fallbackLocale = 'en'}) async {
    if (_initialized) return;

    _lib = _openLibrary();
    _setVerifyKey = _lib
        .lookup<ffi.NativeFunction<_SetVerifyKeyNative>>('l10n4c_set_verify_key')
        .asFunction();
    _setDecryptKey = _lib
        .lookup<ffi.NativeFunction<_SetDecryptKeyNative>>('l10n4c_set_decrypt_key')
        .asFunction();
    _setFallbackLocale = _lib
        .lookup<ffi.NativeFunction<_SetFallbackLocaleNative>>(
          'l10n4c_set_fallback_locale',
        )
        .asFunction();
    _loadLpkLocale = _lib
        .lookup<ffi.NativeFunction<_LoadLpkLocaleNative>>('l10n4c_load_lpk_locale')
        .asFunction();
    _translateAlloc = _lib
        .lookup<ffi.NativeFunction<_TranslateAllocNative>>('l10n4c_translate_alloc')
        .asFunction();
    _translateWithParamsAlloc = _lib
        .lookup<ffi.NativeFunction<_TranslateWithParamsAllocNative>>(
          'l10n4c_translate_with_params_alloc',
        )
        .asFunction();
    _freeString = _lib
        .lookup<ffi.NativeFunction<_FreeStringNative>>('l10n4c_free_string')
        .asFunction();
    _clear = _lib.lookup<ffi.NativeFunction<_ClearNative>>('l10n4c_clear').asFunction();

    _fallback = fallbackLocale;
    _installVerifyKey();
    _installDecryptKeyIfPresent();

    final cFallback = _fallback.toNativeUtf8();
    _setFallbackLocale(cFallback);
    calloc.free(cFallback);

    _initialized = true;
    await loadLocaleFromAsset(_fallback);
  }

  static Future<bool> loadLocaleFromAsset(String locale) async {
    if (!_initialized) return false;
    if (_loadedLocales.contains(locale)) return true;

    try {
      final bytes = await rootBundle.load('assets/locales/$locale.lpk');
      final tempDir = await Directory.systemTemp.createTemp('l10n4x_');
      final lpkFile = File('${tempDir.path}/$locale.lpk');
      await lpkFile.writeAsBytes(bytes.buffer.asUint8List());

      final cLocale = locale.toNativeUtf8();
      final cPath = lpkFile.path.toNativeUtf8();
      final success = _loadLpkLocale(cLocale, cPath) == ok;
      calloc.free(cLocale);
      calloc.free(cPath);

      if (success) {
        _loadedLocales.add(locale);
        return true;
      }
    } on Object {
      return false;
    }
    return false;
  }

  static String translate(
    String locale,
    String key, {
    Map<String, String>? params,
  }) {
    if (!_initialized) return key;

    final cLocale = locale.toNativeUtf8();
    final keyHash = _fnv1a_64(key);
    ffi.Pointer<Utf8> resultPtr;
    final toFree = <ffi.Pointer<Utf8>>[];

    try {
      if (params != null && params.isNotEmpty) {
        final array = calloc<L10n4cParam>(params.length);
        var i = 0;
        for (final entry in params.entries) {
          final ck = entry.key.toNativeUtf8();
          final cv = entry.value.toNativeUtf8();
          toFree.addAll([ck, cv]);
          array[i].key = ck;
          array[i].value = cv;
          i++;
        }
        resultPtr = _translateWithParamsAlloc(cLocale, keyHash, array, params.length);
        calloc.free(array);
      } else {
        resultPtr = _translateAlloc(cLocale, keyHash);
      }

      if (resultPtr.address == 0) return key;
      final text = resultPtr.toDartString();
      _freeString(resultPtr);
      return text;
    } finally {
      calloc.free(cLocale);
      for (final ptr in toFree) {
        calloc.free(ptr);
      }
    }
  }

  static void clear() {
    if (!_initialized) return;
    _clear();
    _loadedLocales.clear();
    _initialized = false;
  }

  static ffi.DynamicLibrary _openLibrary() {
    for (final path in _nativeLibraryCandidates()) {
      if (File(path).existsSync()) {
        return ffi.DynamicLibrary.open(path);
      }
    }

    if (Platform.isAndroid) {
      return ffi.DynamicLibrary.open('libl10n4c.so');
    }

    throw StateError(
      'l10n4c native library not found. '
      'Download a release bundle from $l10n4xReleasesUrl, extract to examples/lib/, '
      'or set L10N4X_LIB_DIR. '
      'Android: copy libl10n4c.so to android/app/src/main/jniLibs/<abi>/.',
    );
  }

  static List<String> _nativeLibraryCandidates() {
    final names = switch (Platform.operatingSystem) {
      'linux' => ['libl10n4c.so', 'libl10n4c-linux.so'],
      'macos' => ['libl10n4c.dylib', 'libl10n4c-macos.dylib'],
      'windows' => ['l10n4c.dll', 'l10n4c-windows.dll'],
      'android' => ['libl10n4c.so', 'libl10n4c-linux.so'],
      _ => <String>[],
    };

    final dirs = <String>[];
    final envDir = Platform.environment['L10N4X_LIB_DIR'];
    if (envDir != null && envDir.isNotEmpty) {
      dirs.add(envDir);
    }
    dirs.add(_examplesLibDir());

    final out = <String>[];
    for (final dir in dirs) {
      for (final name in names) {
        out.add('$dir${Platform.pathSeparator}$name');
      }
    }
    return out;
  }

  static String _examplesLibDir() {
    // When running from examples/flutter, ../lib holds release binaries.
    final cwd = Directory.current.path;
    return '$cwd${Platform.pathSeparator}..${Platform.pathSeparator}lib';
  }

  static void _installVerifyKey() {
    final hex = _verifyKeyFromDefine.isNotEmpty
        ? _verifyKeyFromDefine
        : Platform.environment['L10N4X_VERIFY_PUBLIC_KEY'];
    if (hex == null || hex.isEmpty) {
      throw StateError(
        'Set L10N4X_VERIFY_PUBLIC_KEY via --dart-define or environment variable.',
      );
    }
    final bytes = _hexToBytes(hex);
    final ptr = calloc<ffi.Uint8>(32);
    for (var i = 0; i < 32; i++) {
      ptr[i] = bytes[i];
    }
    if (_setVerifyKey(ptr, 32) != ok) {
      calloc.free(ptr);
      throw StateError('l10n4c: invalid verify public key');
    }
    calloc.free(ptr);
  }

  static void _installDecryptKeyIfPresent() {
    final raw = _encryptKeyFromDefine.isNotEmpty
        ? _encryptKeyFromDefine
        : Platform.environment['L10N4X_ENCRYPT_KEY'];
    if (raw == null || raw.isEmpty) return;
    if (raw.length != 32) {
      throw StateError('L10N4X_ENCRYPT_KEY must be exactly 32 bytes when set');
    }
    final ptr = calloc<ffi.Uint8>(32);
    for (var i = 0; i < 32; i++) {
      ptr[i] = raw.codeUnitAt(i);
    }
    if (_setDecryptKey(ptr, 32) != ok) {
      calloc.free(ptr);
      throw StateError('l10n4c: invalid decrypt key');
    }
    calloc.free(ptr);
  }

  static List<int> _hexToBytes(String hex) {
    if (hex.length != 64) {
      throw StateError('verify public key must be 64 hex characters (32 bytes)');
    }
    final out = <int>[];
    for (var i = 0; i < 32; i++) {
      out.add(int.parse(hex.substring(i * 2, i * 2 + 2), radix: 16));
    }
    return out;
  }
}