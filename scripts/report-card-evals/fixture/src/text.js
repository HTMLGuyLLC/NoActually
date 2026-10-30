export function truncate(text, max) {
  return text.length > max ? text.slice(0, max - 1) + '…' : text;
}
