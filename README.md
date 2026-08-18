## Auto Set Wallpaper And Definition Palette For Theme Linux System

**ASWaDPFTLS** загружает обои, извлекает доминирующую палитру (GPU k-means) и
применяет её к теме системы: пишет `palette.fish` и генерирует конфиги цветов для
набора программ (waybar, swaync, niri, kitty, yazi, walker, nvim, btop, fish).

## Сборка и установка

```bash
cargo build --release          # сборка
cargo install --path .         # установить бинарь в ~/.cargo/bin/ASWaDPFTLS
```

## Использование

```bash
aswadpftls                     # основной режим: выбрать обои и применить тему
aswadpftls help                # справка
```

### Информационные команды

Не меняют систему — только показывают информацию:

```bash
aswadpftls palette <img> [--colors N] [--no-map]
                               # извлечь палитру из картинки и вывести в терминал
                               #   --colors N  число цветов (по умолчанию из конфига)
                               #   --no-map    не показывать раскладку по ролям темы
aswadpftls info                # пути, инструмент, текущие обои и активная палитра
aswadpftls list                # обои хранилища: светлость + мини-палитра (из кэша)
aswadpftls folders             # кластеры хранилища: светлость, когезия, размер
```

## Генераторы конфигов

Для каждой программы из `behavior.generation_conf` запускается
`<scripts_dir>/generation_<name>.fish` (по умолчанию `~/.config/colors/scripts/`).

- Встроенные генераторы (waybar, swaync, niri, kitty, yazi, walker, nvim, btop,
  fish) **авто-генерируются из шаблонов** и помечаются строкой-маркером
  `# ASWADPFTLS-GENERATED`. При обновлении программы такой файл перезаписывается
  из нового шаблона автоматически (раньше устаревший скрипт «залипал»).
- Если содержимое совпадает с шаблоном — файл не трогается, просто запускается.

### Кастомные генераторы

Чтобы добавить свою программу или переопределить встроенную:

```bash
aswadpftls new-generator <name>   # создаёт заготовку generation_<name>.fish
```

Затем добавьте `"<name>"` в `behavior.generation_conf` в `~/.config/colors/config.toml`.

Правила:

- Файл **без** строки-маркера `# ASWADPFTLS-GENERATED` считается кастомным и
  **никогда не перезаписывается**.
- Чтобы кастомизировать встроенный генератор — просто удалите из него строку-маркер.
- Палитра доступна скрипту через переменную окружения `$ASWADPFTLS_PALETTE`:
  ```fish
  source $ASWADPFTLS_PALETTE
  printf 'background %s\n' $COLOR_BASE > ~/.config/myapp/colors.conf
  ```
  Доступны все роли `$COLOR_*` (см. список в заготовке) и их `_ON`-версии
  (контрастный текст на фоне роли).

## Автозапуск (systemd --user)

Юниты лежат в `systemd/`. Установка:

```bash
cargo install --path .
cp systemd/aswadpftls.{service,timer} ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now aswadpftls.timer
```

Таймер запускает программу через минуту после старта сессии и далее каждые
30 минут (палитра следует за временем суток). Если из сервиса не меняются обои —
импортируйте окружение Wayland в user-менеджер:

```bash
systemctl --user import-environment WAYLAND_DISPLAY XDG_RUNTIME_DIR
```
