const MENU_WRITE_MESSAGE = "write-clipboard";

async function writeToClipboard(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (error) {
    console.error("Append Copy: failed to write to clipboard", error);
  }
}

chrome.runtime.onMessage.addListener((message, _sender, _sendResponse) => {
  if (message?.type !== MENU_WRITE_MESSAGE) {
    return;
  }

  const text = typeof message.text === "string" ? message.text.trim() : "";
  if (!text) {
    return;
  }

  void writeToClipboard(text);
});
