# Сторонние компоненты и границы лицензии

Собственный код Crocodile Gena Studio лицензируется автором по **GPL-3.0-only**.
Текст находится в [LICENSE](LICENSE). Это не меняет лицензии сторонних материалов
и не превращает включённые двоичные файлы в открытые исходники.

## Готовые ресурсы

| Файлы / каталог | Что это | Статус прав в этом наборе |
| --- | --- | --- |
| `backend/core_engine/*.dll` | Ресурсы сетевого модуля, включая EOS SDK | Готовые файлы из рабочей версии. Исходники и подтверждение прав на перераспространение в этом каталоге не предоставлены. |
| `CreamAPI/steam_api*.dll` | Ресурсы CreamAPI | Не лицензируются этим проектом по GPL. Условия конкретной версии нужно установить отдельно. |
| `SmokeAPI/SmokeAPI64.dll`, `SmokeAPI86.dll` | Ресурсы SmokeAPI | Имя файла не является подтверждением происхождения или лицензии. Сверить версию и условия с [проектом SmokeAPI](https://github.com/acidicoala/SmokeAPI). |
| `millenium/` | Готовые компоненты Millennium | Проверить версию, исходники и уведомления в [проекте Millennium](https://github.com/SteamClientHomebrew/Millennium). |
| `src-tauri/src/assets/{OpenSteamTool,dwmapi,xinput1_4}.dll` | Ресурсы загрузчика CrocLua | Права и исходники конкретных включённых файлов не подтверждены этим экспортом. |
| DLL из [LeoBanned/pirate-station-hv](https://github.com/LeoBanned/pirate-station-hv) (`standard`, `compatibility`) | Обновляемые комплекты CrocLua | Сторонние компоненты, не авторский код GPL. Проверки формата и контрольных сумм не подтверждают безопасность, лицензию или происхождение DLL. |
| `src-tauri/src/assets/loader-v2/*.dll`, `src-tauri/src/assets/crocodile-manifest/backend/loader/compatibility/*.dll` | Альтернативный комплект CrocLua из `CrocLua/CrocLua2` | Те же ограничения: это сторонние DLL, не авторский код GPL. Факт совместимости на отдельном ПК не подтверждает безопасность, происхождение или право распространения. |
| `src-tauri/resources/hypervisor/Crocodile-HV.exe` | Встроенный лаунчер модуля 05 | Требуется для компиляции через `include_bytes!`. Получен как готовый EXE; исходники и условия его лицензии в проекте не предоставлены. GPL проекта не распространяется на этот файл автоматически. |
| `src-tauri/src/assets/crocodile-manifest/` | Исходники и собранный плагин проекта | Авторский код — GPL-3.0-only; npm-зависимости плагина сохраняют свои лицензии. |

Двоичные ресурсы включены, поскольку код читает их через
`include_bytes!` / `include_str!` или Tauri resources, и без них полная сборка не пройдёт.
В `RESOURCE-MANIFEST.json` экспортёр перечисляет их размеры
и SHA-256. Хэш подтверждает целостность файла, **не безопасность, происхождение или право распространения**.

Перед публичной загрузкой сторонних бинарников владелец проекта должен подтвердить
права и приложить требуемые тексты лицензий/исходники/уведомления. Если разрешения
нет, не публикуйте соответствующий бинарник: сначала замените компонент либо
подготовьте отдельный процесс получения допустимого ресурса. Такая неполная копия
без дополнительных изменений не будет собираться текущей командой.

## Зависимости из Cargo и npm

Точные версии фиксируются в `src-tauri/Cargo.lock` и `package-lock.json`.
Исходники и лицензии зависимостей загружаются из Cargo/npm при сборке.
Tauri, Rust crates, библиотеки React/Millennium и другие зависимости не становятся
GPL только из-за использования в этом приложении.

Особое внимание: `unrar` / `unrar_sys` используют UnRAR для чтения RAR.
Лицензия UnRAR содержит отдельные ограничения. До распространения **связанного
бинарного приложения под GPL** необходимо проверить совместимость и условия
его лицензирования; одна ссылка на этот документ не решает возможную несовместимость.
См. [условия UnRAR у RARLAB](https://www.rarlab.com/rar_add.htm).

Экспорт не является завершённым юридическим аудитом всех компонентов.
# CrocLua local cloud

The application embeds a modified CloudRedirect engine under the MIT license.
Source and license: `third_party/cloudredirect`; upstream:
https://github.com/crghofficial/cloudredirect at commit
`4e81e492e4678c1d907fbeaef2625846f6afaad8`.
The scoped local-only changes and build instructions are documented in that
directory. No upstream WPF companion application is included.
