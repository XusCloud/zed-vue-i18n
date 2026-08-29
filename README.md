# vue-i18n LSP for Zed

`vue-i18n` is a high-performance Language Server Protocol (LSP) implementation designed to provide rich i18n support for Vue.js projects within the Zed editor. It supports Monorepo multi-package architectures, workspace root resolution (including `node_modules` third-party packages), and standard Zed settings configuration.

---

## Features

- **Auto-completion**: Intelligent suggestions for translation keys inside `$t()`, `t()`, or `v-t`.

- **Hover Information**: Quick view of translation values for a given key across configured locales.

- **Go to Definition**: Jump directly to the key definition in JSON/YAML locale files or Vue SFC `<i18n>` blocks.

- **Inlay Hints**: Inline display of translated values next to keys directly in your code buffer.

- **Real-time Diagnostics**: Detect missing keys or formatting errors in real time.

- **Vue SFC `<i18n>` Parsing**: Full support for extracting inline JSON and YAML `<i18n>` blocks from `.vue` Single File Components.

- **Monorepo & Workspace Support**: Automatic resolution of multi-package directory structures and third-party dependency paths (e.g., `node_modules`).

---

## Configuration

The extension is configured directly through Zed's standard `settings.json` (Global or Project-level `.zed/settings.json`) under `initialization_options`.

### Example `settings.json`

```json
{
  "lsp": {
    "vue-i18n": {
      "initialization_options": {
        "defaultLocale": "zh-CN",
        "localeDirs": ["src/locales", "src/i18n"],
        "localeFiles": ["node_modules/element-plus/dist/locale/zh-cn.mjs"],
        "inlayHints": {
          "enabled": true,
          "maxLength": 24
        },
        "packages": [
          {
            "root": "packages/app",
            "defaultLocale": "zh-CN",
            "localeDirs": ["src/locales"]
          },
          {
            "root": "packages/admin",
            "defaultLocale": "en"
          }
        ]
      }
    }
  }
}
```

### Options

| Field                  | Type       | Default                                          | Description                                                                                                        |
| ---------------------- | ---------- | ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `defaultLocale`        | `string`   | `"zh"`                                           | Default target language for inlay hints and quick previews.                                                        |
| `localeDirs`           | `string[]` | `["src/locales", "src/i18n", "locales", "i18n"]` | Directory paths to scan for JSON/YAML translation files.                                                           |
| `localeFiles`          | `string[]` | `[]`                                             | Specific translation file paths (relative to workspace root or package root, e.g., `node_modules` paths).          |
| `inlayHints.enabled`   | `boolean`  | `true`                                           | Enable or disable inline translation hints.                                                                        |
| `inlayHints.maxLength` | `number`   | `24`                                             | Maximum length of rendered inlay hint strings before truncation.                                                   |
| `packages`             | `array`    | `[]`                                             | Sub-package configurations for Monorepo setups. Sub-packages automatically inherit unconfigured parent properties. |

---

## Installation

### Prerequisites

- [Rust](https://www.google.com/search?q=https://rustup.rs/) (Latest stable version)

- [Zed Editor](https://www.google.com/search?q=https://zed.dev/)

### Setup

1. **Clone the repository**:

```bash
git clone https://github.com/XusCloud/zed-vue-i18n.git
cd zed-vue-i18n

```

2. **Build and install the LSP binary**:

```bash

cargo build -p vue-i18n-server --release
cargo build -p vue-i18n-extension --target wasm32-wasip1 --release

```

3. **Install the extension in Zed**:

- Open Zed.

- Open the Command Palette (`Cmd+Shift+P` on macOS or `Ctrl+Shift+P` on Linux/Windows).

- Search and select `zed: install extension from directory`.

- Choose the `extension` directory within the cloned repository.

---
