// Where each patch attempt got to, readable from the CEF debugger as `window.__punktfunkDiag`
// and echoed to the console. Steam's tree is not an API; when it moves, this says which step.
declare global {
  interface Window {
    __punktfunkDiag?: string[];
  }
}

let lastDiag = "";

/** `localStorage["punktfunk:diagVerbose"] = "1"` traces every render step instead of changes. */
export function verbose(): boolean {
  try {
    return localStorage.getItem("punktfunk:diagVerbose") === "1";
  } catch {
    return false;
  }
}

export function diag(msg: string): void {
  if (msg === lastDiag && !verbose()) {
    return; // the page renders several times per open; one line per change is enough
  }
  lastDiag = msg;
  const line = `${new Date().toISOString().slice(11, 19)} ${msg}`;
  console.warn(`punktfunk: ${msg}`);
  try {
    (window.__punktfunkDiag ??= []).push(line);
    if (window.__punktfunkDiag.length > 60) {
      window.__punktfunkDiag.shift();
    }
  } catch {
    /* ignore */
  }
}
