# Собрать Crocodile Gena Studio самостоятельно

Инструкция для **Windows 10/11 x64**, версия проекта **5.5.5**.
GitHub Actions, платный аккаунт GitHub, Python и установленный Steam для сборки не нужны.

## Коротко

Один раз установите Node.js LTS, Rust и C++ Build Tools из следующего раздела.
Скачайте исходники, распакуйте их и откройте PowerShell в папке с `package.json`.

~~~powershell
npm ci
npm run doctor
npm run build:portable
~~~

Готовая программа: **`src-tauri\target\release\crocodile-gena-studio.exe`**.

Хотите без ввода команд? После установки инструментов дважды нажмите
**`BUILD-WINDOWS.cmd`** в корне проекта. Он установит npm-зависимости, проверит
файлы и соберёт этот же EXE. Окно покажет результат и не закроется сразу при ошибке.

## 1. Установите инструменты — один раз

1. **[Node.js LTS](https://nodejs.org/en/download)** с npm. Для этой инструкции
   используйте LTS 22 или новее. Отдельно npm устанавливать не нужно.
2. **[Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)**.
   В установщике выберите **«Разработка классических приложений на C++» /
   Desktop development with C++**. Оставьте включёнными MSVC для x64/x86
   и Windows 10/11 SDK. Полная Visual Studio не обязательна.
3. **[Rust через rustup](https://rustup.rs/)**. Установите стандартный
   набор stable для **`x86_64-pc-windows-msvc`**.
4. **[WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)**:
   нужен для окна приложения. Если уже установлен, повторная установка не нужна.
   Если отсутствует, используйте Evergreen Bootstrapper.

После установки **закройте и снова откройте PowerShell**.
При необходимости завершите перезагрузку, которую запросил установщик.

Проверка:

~~~powershell
node --version
npm --version
rustc --version
cargo --version
rustup show active-toolchain
~~~

Последняя команда должна показывать x64 MSVC. Если выбран GNU или другая платформа:

~~~powershell
rustup default stable-x86_64-pc-windows-msvc
~~~

Инструменты устанавливайте с официальных сайтов. Требования Windows описаны
также в [официальной инструкции Tauri](https://v2.tauri.app/start/prerequisites/).
Для первой сборки нужен интернет; зависимости и инструменты упаковки скачиваются автоматически.
Предусмотрите несколько гигабайт свободного места под Rust-зависимости и `target`.

## 2. Скачайте исходники

На странице GitHub нажмите **Code → Download ZIP**, затем распакуйте ZIP целиком.
Не запускайте сборку прямо из архиватора. Лучше выбрать короткий путь, например
`C:\Projects\CrocodileGenaStudio`.

Откройте эту папку в Проводнике, в адресной строке введите `powershell` и нажмите Enter.
В открывшемся терминале команда `dir package.json` должна находить файл.

Если уже установлен Git, вместо ZIP можно выполнить `git clone`, используя
адрес **своего выбранного репозитория**, и перейти в скачанную папку.
Git LFS для подготовленного экспортного набора не требуется.

## 3. Соберите нужный вариант

### Только EXE, без установки

~~~powershell
npm ci
npm run doctor
npm run build:portable
~~~

Результат:

~~~text
src-tauri\target\release\crocodile-gena-studio.exe
~~~

Для запуска этого EXE не нужны Node.js и Rust. Нужен WebView2 Runtime.
Ресурсы программы встроены в сборку; `node_modules` и `target` целиком передавать
пользователям не нужно.

### Установщик EXE — NSIS

После `npm ci` и проверки:

~~~powershell
npm run build:setup
~~~

Результат для версии 5.5.5:

~~~text
src-tauri\target\release\bundle\nsis\Crocodile Gena Studio_5.5.5_x64-setup.exe
~~~

NSIS-установщик настроен на загрузку WebView2 при необходимости, поэтому
его первая установка может требовать интернет.

### EXE + NSIS + MSI

~~~powershell
npm run build -- --ci
~~~

Дополнительно появится:

~~~text
src-tauri\target\release\bundle\msi\Crocodile Gena Studio_5.5.5_x64_en-US.msi
~~~

Для MSI могут понадобиться включённые компоненты VBScript в Windows.
Если нужен только EXE или NSIS, выбирайте предыдущие команды:
сборка MSI тогда не запускается. Подробнее:
[упаковка Windows-приложений Tauri](https://v2.tauri.app/distribute/windows-installer/).

### Через помощник PowerShell

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\build-windows.ps1 -Target portable
~~~

Вместо `portable` можно указать `setup` или `all`.
`Bypass` действует только в этом процессе; системная политика не меняется.
Помощник не устанавливает Rust или Visual Studio и не изменяет настройки защиты Windows.

Первый запуск может быть заметно дольше повторного.
Последняя стадия оптимизации Rust иногда несколько минут не печатает новых строк —
это само по себе не означает зависание.

## 4. Ключи сервисов

**Для сборки ключи не нужны.** В публичных исходниках нет личного ключа разработчика.

- **HubCap:** свой ключ вводится в настройках уже запущенной программы.
- **Ryuu:** если у вас есть разрешённый ключ, перед сборкой задайте его
  в текущем PowerShell. В следующей команде замените пример своим значением:

~~~powershell
$env:CGS_RYUU_API_KEY = 'YOUR_OWN_KEY'
npm run build:portable
~~~

После сборки можно убрать переменную из текущего терминала:

~~~powershell
Remove-Item Env:\CGS_RYUU_API_KEY -ErrorAction SilentlyContinue
~~~

Без ключа Ryuu приложение компилируется; загрузка через Ryuu объяснит, что ключ не задан.
HubCap, локальный импорт и другие модули не требуют этого ключа.

Ключ, заданный при сборке, встраивается в EXE и может быть извлечён.
Не публикуйте секретные значения в GitHub, `.env`, документации или общих EXE.
Плагин Millennium имеет собственные настройки ключей.

## 5. Какие папки нельзя потерять

В публичном наборе названия приведены к такому виду:

~~~text
package.json
package-lock.json
ui/                         # интерфейс и изображения
src-tauri/
  Cargo.toml
  Cargo.lock
  build.rs
  tauri.conf.json
  icons/
  capabilities/
  src/assets/
    crocodile-manifest/
      .millennium/Dist/     # index.js и webkit.js: нужны для компиляции
backend/core_engine/        # готовые ресурсные DLL
CreamAPI/
  steam_api.dll
  steam_api64.dll
SmokeAPI/
  SmokeAPI64.dll
  SmokeAPI86.dll
millenium/                  # это фактическое имя каталога в коде
tools/
tests/
LICENSE
THIRD_PARTY.md
~~~

`include_bytes!` и `include_str!` читают ресурсы при компиляции.
Поэтому исходников `.rs` и `ui` без перечисленных ресурсов недостаточно.
Помощник `npm run doctor` проверяет конкретные пути из Rust-кода и конфигурации Tauri.

**Четыре файла гипервизора не нужны для сборки:** `HV-Launcher.bat`,
`HV-PlugNPlay.bat`, `kvc.exe` и `drvloader.exe`. Публичный экспорт их не содержит.
Программа собирается и запускается без них. Если профиля нет также в локальных
файлах/кэше, запуск соответствующего профиля покажет сообщение об отсутствии ресурсов.
Остальные модули доступны. Для полной собственной сборки разрешённые файлы можно
положить в `src-tauri/resources/hypervisor` перед компиляцией — сборщик встроит их автоматически.

Имя с точкой, например `.millennium`, **не делает папку автоматически скрытой в Windows**.
Однако некоторые способы выборочного копирования пропускают такие каталоги.
Скачивание всего репозитория через Git или Download ZIP сохраняет их.

В личной рабочей папке автора могут оставаться старые имена `DLCUnlocker` и `СмакиАпи`.
Экспортёр преобразует их в `CreamAPI` и `SmokeAPI` и обновляет ссылки **в экспортной копии**.
В скачанном публичном репозитории вручную переименовывать ничего не нужно.

## 6. Если что-то не получилось

| Сообщение / ситуация | Что проверить |
| --- | --- |
| `npm` не найден | Установить Node.js, затем заново открыть терминал. |
| PowerShell блокирует `npm.ps1` | Выполнить `npm.cmd ci`, `npm.cmd run doctor`, `npm.cmd run build:portable`. Либо использовать `BUILD-WINDOWS.cmd`. |
| `cargo` / `rustc` не найден | Установить rustup и снова открыть терминал. |
| `link.exe not found` или отсутствует Windows SDK | В Visual Studio Installer добавить C++ workload, MSVC и Windows SDK. |
| `couldn't read ...`, `resource path ... doesn't exist` | Выполнить `npm run doctor`. Заново скачать полный ZIP; не создавать пустые DLL вместо потерянных файлов. |
| `npm ci` сообщает несовпадение lockfile | Использовать `package.json` и `package-lock.json` из одной версии репозитория. Для намеренного изменения зависимостей обновить lockfile через `npm install`. |
| Не скачиваются crates / NSIS / WebView2 | Проверить доступ к registry.npmjs.org, crates.io и GitHub. Повторить сборку после восстановления сети. |
| MSI падает на `light.exe` | Собрать `npm run build:setup` без MSI или проверить компонент VBScript по ссылке Tauri выше. |
| Окно программы не открывается | Проверить установку WebView2 Runtime и сообщения приложения. |
| Защита удалила ресурс | Проверить карантин, источник и хэш из `RESOURCE-MANIFEST.json`. Не отключать защиту целиком и не брать случайные DLL с посторонних сайтов. |

Для сборки **не нужно** отключать Secure Boot, VBS, DSE или включать тестовый режим.
Эти настройки относятся к отдельным функциям приложения, а не к компилятору.

## 7. Запуск для разработки и тесты

~~~powershell
npm run dev
~~~

Интерфейс здесь — обычные HTML/CSS/JS из `ui`, отдельная сборка Vite не нужна.
Плагин Millennium уже содержит необходимые собранные JS.
Для изменения самого плагина см. его README в
`src-tauri/src/assets/crocodile-manifest`.

Быстрые тесты интерфейсной логики, без установки файлов в игры:

~~~powershell
npm test
~~~

Тесты Rust:

~~~powershell
npm run test:native
~~~

Для версии из экспортной папки проверены установка npm-зависимостей,
проверка ресурсов, эти тесты и команда сборки EXE.
Результат ручной сборки не обязан побайтово совпадать с официальным EXE:
версии инструментов, ключи и параметры окружения могут отличаться.

## Лицензия

Собственный код автора опубликован под **GPL-3.0-only** — см. [LICENSE](LICENSE).
Готовые сторонние DLL, EXE, UnRAR и прочие зависимости имеют отдельные условия;
их включение не означает передачу прав на них по GPL.
