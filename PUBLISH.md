# Что загрузить на GitHub

Публикуйте **содержимое `github_export`**, а не рабочую папку проекта целиком
и не сам каталог `github_export` как вложенную папку репозитория.
`package.json`, `README.md`, `BUILD.md`, `LICENSE` и `src-tauri` должны быть
на верхнем уровне репозитория.

1. Прочитайте [THIRD_PARTY.md](THIRD_PARTY.md): права на сторонние DLL/EXE нужно
   подтвердить перед их публичной загрузкой. GPL-3.0-only применяется к собственному коду автора.
2. Загрузите весь подготовленный набор, включая `.gitignore` и
   `src-tauri/src/assets/crocodile-manifest/.millennium/Dist`.
3. Не добавляйте старые EXE/установщики, `node_modules`, `target`, личные ключи
   и резервную копию предыдущего экспорта. Готовые релизы размещаются отдельно в Releases.
4. Проверьте страницу репозитория: читатель должен сразу увидеть ссылку на `BUILD.md`.

## Загрузка через Git — рекомендуемый способ

Откройте PowerShell именно в `github_export`. Если Git ещё не установлен,
используйте [Git for Windows](https://git-scm.com/download/win).

~~~powershell
git init
git add .
git status --short
~~~

Проверьте список добавленных файлов. Затем:

~~~powershell
git commit -m "Publish Crocodile Gena Studio 5.5.6 sources"
git branch -M main
git remote add origin https://github.com/YOUR_ACCOUNT/YOUR_REPOSITORY.git
git push -u origin main
~~~

В адресе замените `YOUR_ACCOUNT` и `YOUR_REPOSITORY` своими значениями.
Создайте **пустой репозиторий** перед первым push. Эти команды предназначены для
нового репозитория, не для перезаписи существующей истории.

Если загружаете через сайт, включите показ скрытых файлов и отдельно убедитесь,
что `.millennium/Dist` и `.gitignore` действительно загрузились.
Git удобнее для большого числа файлов и сохранения структуры.

Экспортёр проверяет, что отдельные файлы меньше 100 MiB — предела GitHub для обычного push.
Подробнее: [ограничения размера файлов GitHub](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github).
Workflow для GitHub Actions в набор намеренно не добавляется: инструкция рассчитана
на локальную сборку, которая не расходует минуты Actions.

## Обновить экспорт позже

В рабочей папке проекта:

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\Export-GitHub.ps1
~~~

Экспортёр копирует актуальные исходники и ресурсы, переименовывает пути CreamAPI/SmokeAPI
в экспортной копии и удаляет встроенный личный ключ Ryuu из этой копии.
Он не изменяет работающую программу, Steam или файлы игр.
Предыдущий экспорт сохраняется рядом как `github_export.previous-...` для восстановления.

После публикации версии проверяйте, что соответствующие исходники и тексты лицензий
доступны получателям сборки. Не публикуйте EXE, собранный с личным секретным ключом.
