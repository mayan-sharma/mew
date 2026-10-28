// Mew runs on Windows and macOS; a few words and shortcuts differ.

export const mac = navigator.userAgent.includes("Mac");
export const device = mac ? "Mac" : "PC";
