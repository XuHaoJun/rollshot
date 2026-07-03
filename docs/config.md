# Rollshot Configuration

Rollshot uses one TOML configuration file:

- Linux: `$XDG_CONFIG_HOME/rollshot/config.toml`, normally
  `~/.config/rollshot/config.toml`
- macOS: `~/Library/Application Support/rollshot/config.toml`

Every section is optional. Missing sections and missing files use platform
defaults.

## Example

```toml
[daemon]
capture_region_hotkey = "Alt+Shift+6"

[provider]
provider = "Anthropic"
model = "claude-sonnet-4-6"
key_source = { Env = "ANTHROPIC_API_KEY" }
# base_url = "https://api.anthropic.com"
```

## `[daemon]`

Daemon settings control the tray/menu-bar daemon.

### `capture_region_hotkey`

Global shortcut that starts region capture from the daemon.

- Type: string
- Linux default: `"Alt+Shift+6"`
- macOS default: `"Command+Shift+6"`
- Required: no

Shortcut syntax:

- Components are separated with `+`.
- At least one modifier is required.
- The base key must be one ASCII letter, one ASCII digit, or `F1` through
  `F24`.
- Supported modifier names are `Control`/`Ctrl`, `Alt`/`Option`, `Shift`,
  `Command`/`Cmd`, and `Super`/`Logo`.
- Duplicate modifiers are rejected.
- `Command` and `Super` cannot be used together because they name the same
  platform modifier.

Examples:

```toml
[daemon]
capture_region_hotkey = "Control+Alt+7"
```

```toml
[daemon]
capture_region_hotkey = "Command+Shift+F6"
```

## `[provider]`

Provider settings control the Smart Redaction LLM provider. API key values are
not stored in the config file; Rollshot stores only the environment variable
name and resolves the value at runtime.

### `provider`

LLM provider adapter to use.

- Type: enum string
- Default: `"Anthropic"`
- Required: no
- Supported values: `"Anthropic"`, `"OpenAI"`

### `model`

Model name sent to the configured provider.

- Type: string
- Default: `"claude-sonnet-4-6"`
- Required: no

The value is passed through to the provider adapter. Use a model name accepted
by the selected provider or by the configured compatible endpoint.

### `base_url`

Provider API base URL override.

- Type: string
- Default for `Anthropic`: `"https://api.anthropic.com"`
- Default for `OpenAI`: `"https://api.openai.com/v1"`
- Required: no

Set this when using a compatible local proxy or non-default endpoint.

Example:

```toml
[provider]
provider = "Anthropic"
model = "claude-sonnet-4-6"
base_url = "http://127.0.0.1:8080"
key_source = { Env = "ANTHROPIC_API_KEY" }
```

### `key_source`

Source used to resolve the provider API key at runtime.

- Type: table
- Default: `{ Env = "ANTHROPIC_API_KEY" }`
- Required: no
- Supported forms: `{ Env = "<ENV_VAR_NAME>" }`

Rollshot reads the named environment variable when starting a provider-backed
run. Empty or missing environment variables are treated as unavailable keys.

Examples:

```toml
[provider]
provider = "Anthropic"
model = "claude-sonnet-4-6"
key_source = { Env = "ANTHROPIC_API_KEY" }
```

```toml
[provider]
provider = "OpenAI"
model = "gpt-4o"
key_source = { Env = "OPENAI_API_KEY" }
```
