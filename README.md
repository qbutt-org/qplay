<p align="center">
  <strong>Русский</strong> / <a href="README.en.md">English</a>
</p>

<h1 align="center">qplay</h1>

<p align="center">
  Видеоплеер на основе <a href="https://mpv.io/">mpv</a>.
</p>

<p align="center">
  <a href="https://github.com/qbutt-org/qplay/releases/latest/download/qplay-setup.exe">
    <img alt="Скачать qplay для Windows" src="https://img.shields.io/badge/%D0%A1%D0%9A%D0%90%D0%A7%D0%90%D0%A2%D0%AC-Windows-009df7?style=for-the-badge">
  </a>
  <a href="https://github.com/qbutt-org/qplay/releases">
    <img alt="Все релизы" src="https://img.shields.io/badge/%D0%A0%D0%95%D0%9B%D0%98%D0%97%D0%AB-%D0%B2%D1%81%D0%B5_%D0%B2%D0%B5%D1%80%D1%81%D0%B8%D0%B8-24292f?style=for-the-badge&amp;logo=github&amp;logoColor=white">
  </a>
</p>

<p align="center">
  <img alt="Windows x64" src="https://img.shields.io/badge/Windows-x64-0078d4?style=flat-square">
  <a href="https://github.com/qbutt-org/qplay/releases/latest">
    <img alt="Последний релиз" src="https://img.shields.io/github/v/release/qbutt-org/qplay?label=release&amp;color=009df7&amp;style=flat-square">
  </a>
  <a href="LICENSE">
    <img alt="GPL" src="https://img.shields.io/badge/license-GPL-blue?style=flat-square">
  </a>
</p>

<p align="center">
  <a href="https://qbutt-org.github.io/qplay/">Сайт</a> /
  <a href="https://github.com/qbutt-org/qplay/releases/latest/download/qplay-portable.zip">Portable</a>
</p>

<p align="center">
  <img src="docs/assets/player-ru.png" alt="qplay с открытым видео и панелью управления" width="960">
  <br>
  <sub>Кадр из Sintel. © Blender Foundation / <a href="https://durian.blender.org/">durian.blender.org</a>, <a href="https://creativecommons.org/licenses/by/3.0/">CC BY 3.0</a>.</sub>
</p>

## Установка

Запустите **qplay-setup.exe**. Поддерживаются Windows 10 версии 2004 и новее и Windows 11, x64. qplay появляется в меню «Пуск» и в списке приложений Windows.

Для portable-версии распакуйте **qplay-portable.zip** и запустите `qplay.exe`.

Установленная версия скачивает обновления в фоне и устанавливает их после закрытия всех окон плеера. Portable-версия обновляется вручную из настроек.

Откройте медиафайл или прямую ссылку в окне плеера. qplay появляется в меню Windows **Открыть с помощью** для распространённых форматов видео и аудио. Приложение не меняет выбранный вами плеер по умолчанию.

## Управление и изображение

Кнопка параметров справа внизу открывает выбор аудиодорожки, субтитров и скорости. Там же доступны обработка изображения и генерация кадров. Режимы обработки зависят от видеокарты и самого видео.

Субтитры можно выключить, выбрать из дорожек видео или добавить отдельным файлом. Плеер запоминает место остановки и продолжает просмотр при повторном открытии.

Для обычного воспроизведения достаточно установленного комплекта. Для GPU-режимов плеер может автоматически скачать и подготовить дополнительные компоненты.

## qbutt и qcatch

[qbutt](https://github.com/qbutt-org/qbutt) использует qplay для онлайн-видео и просмотра торрентных файлов во время загрузки. Установщик qbutt устанавливает этот же плеер.

[qcatch](https://github.com/qbutt-org/qcatch) находит видео в браузере и открывает выбранный поток в установленном qplay.

## Настройки

Установленная версия хранит настройки в `%APPDATA%\qplay`, а кеш и подготовленные GPU-компоненты — в `%LOCALAPPDATA%\qplay`. Portable-версия использует папку `userdata` рядом с приложением. Существующие настройки mpv и mpv.net не заменяются.

В расширенных настройках можно сменить язык и оформление, а также задать постоянную задержку звука относительно видео. Выбор обработки изображения сохраняется между запусками.

## Разработка

Нативный плеер находится в `src/native/`. QML отвечает за интерфейс, Rust — за сессию libmpv и настройки. Закреплённые зависимости и происхождение runtime записаны в `upstream-lock.json`.

Для локальной сборки нужны Qt 6.12, CMake, MSVC, Rust из `rust-toolchain.toml` и Bun. Из Developer PowerShell соберите CMake target `qplay-native`, затем укажите каталог сборки и Inno Setup 6.7.3:

```powershell
bun scripts/build.ts --native-build "C:\build\qplay-native" --installer-compiler "C:\tools\Inno Setup 6\ISCC.exe"
```

Скрипт проверяет исходную ревизию native build, пересобирает изменившиеся файлы, сверяет закреплённый runtime, развёртывает Qt/QML и создаёт установщик и portable-архив в `%TEMP%\qplay-build`. Параметры `--runtime-archive` и `--output` позволяют указать готовый архив и каталог результата.

Команды `qplay.exe --version-json` и `qplay.exe --runtime-info-json` возвращают метаданные для интеграций. Плеер принимает путь к файлу или URL отдельным аргументом.

См. [правила проекта](AGENTS.md).

## Лицензии и происхождение

Комплект включает Qt-интерфейс, шейдеры и медиазависимости с уведомлениями авторов. Лицензии находятся в [licenses/](licenses/), а условия исходников qplay — в [LICENSE](LICENSE).

Исходники qplay доступны в этом репозитории. Полные исходники медиадвижка и его зависимостей для версии 1.0.0 опубликованы в [отдельном техническом релизе](https://github.com/qbutt-org/qplay/releases/tag/v1.0.0-sources).
