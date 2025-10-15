const STORAGE_KEY = "appendCopyClipboard";
const DEFAULT_DELIMITER = "\n";
const MENU_ID = "append-copy-selection";
const WRITE_MESSAGE = "write-clipboard";

function ensureStorageInitialized() {
  chrome.storage.local.get(STORAGE_KEY, (stored) => {
    if (chrome.runtime.lastError) {
      console.error("Storage read failed on install", chrome.runtime.lastError);
      return;
    }
    if (typeof stored[STORAGE_KEY] === "undefined") {
      chrome.storage.local.set({ [STORAGE_KEY]: "" });
    }
  });
}

function createContextMenu() {
  chrome.contextMenus.removeAll(() => {
    if (chrome.runtime.lastError) {
      console.error("Failed to clear context menus", chrome.runtime.lastError);
      return;
    }

    chrome.contextMenus.create(
      {
        id: MENU_ID,
        title: "Append selection to clipboard stack",
        contexts: ["selection"]
      },
      () => {
        if (chrome.runtime.lastError) {
          console.error("Failed to create context menu", chrome.runtime.lastError);
        }
      }
    );
  });
}

function initializeExtensionState() {
  ensureStorageInitialized();
  createContextMenu();
}

chrome.runtime.onInstalled.addListener(() => {
  initializeExtensionState();
});

chrome.runtime.onStartup.addListener(() => {
  initializeExtensionState();
});

function appendSelection(text, delimiter, callback) {
  chrome.storage.local.get(STORAGE_KEY, (result) => {
    if (chrome.runtime.lastError) {
      console.error("Storage read failed", chrome.runtime.lastError);
      callback({ success: false, reason: "storage-read" });
      return;
    }

    let combined = result?.[STORAGE_KEY] || "";
    combined = combined ? `${combined}${delimiter}${text}` : text;

    chrome.storage.local.set({ [STORAGE_KEY]: combined }, () => {
      if (chrome.runtime.lastError) {
        console.error("Storage write failed", chrome.runtime.lastError);
        callback({ success: false, reason: "storage-write" });
        return;
      }

      callback({ success: true, combined });
    });
  });
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== MENU_ID) {
    return;
  }

  const rawText = typeof info.selectionText === "string" ? info.selectionText : "";
  const text = rawText.trim();
  if (!text) {
    return;
  }

  const delimiter = DEFAULT_DELIMITER;

  appendSelection(text, delimiter, (result) => {
    if (!result.success || !result.combined) {
      return;
    }

    if (tab?.id === undefined) {
      return;
    }

    chrome.tabs.sendMessage(tab.id, { type: WRITE_MESSAGE, text: result.combined }, () => {
      if (chrome.runtime.lastError) {
        console.error("Failed to notify content script", chrome.runtime.lastError);
      }
    });
  });
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "clear-clipboard") {
    return;
  }

  chrome.storage.local.set({ [STORAGE_KEY]: "" }, () => {
    if (chrome.runtime.lastError) {
      console.error("Failed to clear clipboard store", chrome.runtime.lastError);
      sendResponse({ success: false, reason: "storage-write" });
      return;
    }

    sendResponse({ success: true, combined: "" });
  });

  return true;
});
