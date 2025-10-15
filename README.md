# Reveal

Reveal is a simple, lightweight, and fast tool for League of Legends to reveal your team mates names in champ select. We also have dodging and a auto acceptor.

### Features

- Reveal your team mates names in champ select
- Auto acceptor
- Dodging & Last Second Dodging

## Chrome extension: Append Copy

The repository also contains a standalone Manifest V3 Chrome extension inside the [`extension/`](extension) directory. The extension appends any selected text to a running clipboard stack when you activate its context menu item. Each new selection is appended to the bottom of the existing clipboard contents instead of replacing it.

### Loading the extension locally

1. Open `chrome://extensions` in Chromium-based browsers.
2. Enable **Developer mode** in the top right corner.
3. Choose **Load unpacked** and select the [`extension/`](extension) folder from this project.
4. Navigate to any page, highlight some text, open the context menu, and choose **Append selection to clipboard stack** to append the selection to your clipboard stack.

The extension stores the aggregated clipboard text locally, so it remembers previous entries until the background service worker is restarted or the stored data is cleared.
