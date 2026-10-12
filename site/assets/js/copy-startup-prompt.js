// Optional one-click copy for the static first-message prompt.
const copyPromptButton = document.getElementById("copy-jelly-startup-prompt");
const startupPrompt = document.getElementById("jelly-startup-prompt");
const copyPromptStatus = document.getElementById("jelly-startup-copy-status");

if (copyPromptButton && startupPrompt && copyPromptStatus) {
  copyPromptButton.addEventListener("click", async () => {
    const prompt = startupPrompt.textContent.trim();

    try {
      await navigator.clipboard.writeText(prompt);
      copyPromptStatus.textContent = "Copied. Paste it into your AI chat.";
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(startupPrompt);
      selection?.removeAllRanges();
      selection?.addRange(range);
      copyPromptStatus.textContent =
        "Automatic copying was unavailable. Copy the selected text.";
    }
  });
}
