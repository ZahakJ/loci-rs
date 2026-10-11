/** `Parent::Child` → `Child.apkg`, without characters file systems dislike (pure; tested). */
export function exportFileName(deckName: string): string {
  const leaf = deckName.split('::').pop() ?? deckName;
  return `${leaf.replace(/[^\p{L}\p{N} _.-]+/gu, '').replace(/^\.+/, '').trim() || 'deck'}.apkg`;
}
