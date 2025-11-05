# Browser Panel

A browser panel for Zed that displays web content.

## Features

### Default Mode (Lightweight)

By default, the browser panel provides a UI mockup with:
- Address bar with URL input
- Navigation controls (back, forward, reload)
- History management
- Visual feedback for navigation

This mode has **no external dependencies** and adds minimal build time.

### Servo Browser Mode (Optional)

Enable the `servo-browser` feature to get full web page rendering using the Servo engine.

## Building with Servo

### Prerequisites

**System Dependencies (Ubuntu/Debian):**
```bash
sudo apt-get install -y \
    cmake \
    libfreetype6-dev \
    libgl1-mesa-dev \
    libglib2.0-dev \
    libharfbuzz-dev \
    libssl-dev \
    libx11-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    python3
```

**System Dependencies (Fedora/RHEL):**
```bash
sudo dnf install -y \
    cmake \
    freetype-devel \
    mesa-libGL-devel \
    glib2-devel \
    harfbuzz-devel \
    openssl-devel \
    libX11-devel \
    libxcb-devel \
    python3
```

**System Dependencies (macOS):**
```bash
brew install cmake python3
```

### Building Zed with Servo

```bash
# Build with Servo browser engine
cargo build --features browser_panel/servo-browser

# Or for release builds
cargo build --release --features browser_panel/servo-browser
```

### Build Time Expectations

- **First build with Servo**: 30-60 minutes (depending on machine)
- **Subsequent builds**: ~5-10 minutes for incremental changes
- **Binary size increase**: ~100-150 MB
- **Default build (without Servo)**: No impact

### Updating Servo Version

To update to a newer Servo commit:

1. Find the latest commit hash: https://github.com/servo/servo/commits/main
2. Update `crates/browser_panel/Cargo.toml`:
   ```toml
   libservo = { git = "https://github.com/servo/servo", rev = "NEW_COMMIT_HASH", optional = true, package = "servo" }
   ```
3. Run `cargo update -p servo`

## Architecture

### Default Mode

```
┌─────────────────────────┐
│   Browser Panel UI      │
│  ┌──────────────────┐   │
│  │  Address Bar     │   │
│  └──────────────────┘   │
│  ┌──────────────────┐   │
│  │  [Navigation]    │   │
│  └──────────────────┘   │
│  ┌──────────────────┐   │
│  │  Status Display  │   │
│  └──────────────────┘   │
└─────────────────────────┘
```

### Servo Mode

```
┌───────────────────────────────┐
│   Browser Panel UI            │
│  ┌────────────────────────┐   │
│  │  Address Bar           │   │
│  └────────────────────────┘   │
│  ┌────────────────────────┐   │
│  │  [Navigation]          │   │
│  └────────────────────────┘   │
│  ┌────────────────────────┐   │
│  │  Servo Engine          │   │
│  │   ↓                    │   │
│  │  Offscreen Renderer    │   │
│  │   ↓                    │   │
│  │  OpenGL Texture        │   │
│  │   ↓                    │   │
│  │  GPUI Image Element    │   │
│  └────────────────────────┘   │
└───────────────────────────────┘
```

## Implementation Status

- [x] UI mockup and controls
- [x] URL normalization and history
- [x] Feature flag infrastructure
- [ ] Servo offscreen rendering
- [ ] Texture sharing with GPUI
- [ ] Input event forwarding
- [ ] Full navigation integration

## Contributing

When contributing to the browser panel:

1. Ensure changes work **without** the servo-browser feature
2. Test with servo-browser feature enabled if you modified Servo integration
3. Keep servo-related code behind `#[cfg(feature = "servo-browser")]`
