type NavigableItem = { disabled?: boolean };

export function findEnabledIndex(
  items: NavigableItem[],
  start: number,
  direction: 1 | -1,
  includeStart = true
) {
  if (!items.length) return -1;
  let index = includeStart ? start : start + direction;
  for (let visited = 0; visited < items.length; visited += 1) {
    index = (index + items.length) % items.length;
    if (!items[index]?.disabled) return index;
    index += direction;
  }
  return -1;
}
