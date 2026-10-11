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

Откройте медиафайл в qplay, перетащите его в окно или вставьте прямую ссылку в начальном окне. qplay появляется в меню Windows **Открыть с помощью** для распространённых форматов видео и аудио. Приложение не заменяет выбранный вами плеер по умолчанию.

## Управление и изображение

В меню FX можно выбрать обработку изображения. Апскейл и генерация кадров используют видеокарту; доступные режимы и производительность зависят от оборудования.

Нажмите значок субтитров, чтобы выбрать дорожку или выключить её. Выберите **Переместить субтитры**, перетащите ручку ползунка и нажмите **Готово**. Положение сохраняется между запусками. **Автоматическая позиция субтитров** возвращает стандартное размещение. У ASS-субтитров с заданными координатами перемещение ограничено.

Для обычного воспроизведения достаточно установленного комплекта. Для GPU-режимов плеер может автоматически скачать и подготовить дополнительные компоненты.

## qbutt и qcatch

[qbutt](https://github.com/qbutt-org/qbutt) использует qplay для онлайн-видео и просмотра торрентных файлов во время загрузки. Установщик qbutt устанавливает этот же плеер.

[qcatch](https://github.com/qbutt-org/qcatch) находит видео в браузере и открывает выбранный поток в установленном qplay.

## Настройки

Установленная версия хранит настройки в `%APPDATA%\qplay`, а кеш и подготовленные GPU-компоненты — в `%LOCALAPPDATA%\qplay`. Portable-версия использует папку `userdata` рядом с приложением. Существующие настройки mpv и mpv.net не заменяются.

В настройках можно выбрать язык, продолжение с места остановки и автоматическую генерацию кадров. Там же проверяются обновления: установленная версия скачивает проверенный EXE, portable-версия предлагает новый архив.

qplay принимает обычные параметры mpv. Путь к движку и собственному профилю определяет плеер.

## Разработка

Исходники Windows-лаунчера находятся в `src/`, подготовленный профиль плеера — в `profile/`. Закреплённые зависимости и происхождение runtime записаны в `upstream-lock.json`.

Локальная сборка использует Bun 1.4.2 и встроенный компилятор .NET Framework. .NET SDK не требуется. При проверке релиза использована команда ниже; укажите свои пути к закреплённому архиву runtime и `ISCC.exe` из Inno Setup 6.7.3:

```powershell
bun scripts/build.ts --runtime-archive "C:\build\qplay-1.0.0-runtime.zip" --installer-compiler "C:\tools\Inno Setup 6\ISCC.exe"
```

Скрипт проверяет SHA-256 runtime и создаёт установщик и portable-архив в `%TEMP%\qplay-build`; другой каталог задаётся через `--output`. Режим `bun run build` с автоматической загрузкой зависимостей предусмотрен, но подготовка Inno Setup в этом режиме пока не проверена.

Команды `qplay.exe --version-json` и `qplay.exe --runtime-info-json` возвращают метаданные для интеграций. Лаунчер остаётся жив до завершения своего движка и отвечает за его жизненный цикл. Вызывающее приложение передаёт медиа и параметры воспроизведения.

См. [правила проекта](AGENTS.md).

## Лицензии и происхождение

Комплект включает ModernX, Lua-скрипты, шейдеры и медиазависимости с сохранёнными уведомлениями авторов. Лицензии находятся в [licenses/](licenses/), а условия исходников qplay — в [LICENSE](LICENSE).

Исходники qplay доступны в этом репозитории. Полные исходники медиадвижка и его зависимостей для версии 1.0.0 опубликованы в [отдельном техническом релизе](https://github.com/qbutt-org/qplay/releases/tag/v1.0.0-sources).
