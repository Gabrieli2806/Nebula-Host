/**
 * Copy text, also where the Clipboard API is missing: the web panel opened
 * from a phone at http://192.168.x.x is not a secure context, so
 * `navigator.clipboard` does not exist there. The old way still works.
 */
export async function copyText(text: string): Promise<void> {
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Refused (permissions, unfocused): try the old way.
    }
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  area.style.top = "0";
  document.body.appendChild(area);
  area.select();
  area.setSelectionRange(0, text.length);
  const done = document.execCommand("copy");
  area.remove();
  if (!done) throw new Error("copy refused");
}
