export function boardCoordinates(flipped) {
  const ranks = flipped ? [0, 1, 2, 3, 4, 5, 6, 7] : [7, 6, 5, 4, 3, 2, 1, 0];
  const files = flipped ? [7, 6, 5, 4, 3, 2, 1, 0] : [0, 1, 2, 3, 4, 5, 6, 7];
  return ranks.flatMap((rank, row) => files.map((file, column) => ({ rank, file, row, column })));
}

export function perspectivePlayerIsBlack(player, managedUser, black) {
  const perspectivePlayer = player ?? managedUser;
  return Boolean(
    perspectivePlayer
      && black
      && perspectivePlayer.toLowerCase() === black.toLowerCase(),
  );
}

export function containedScrollDelta(container, item, padding = 8) {
  if (item.top < container.top) return item.top - container.top - padding;
  if (item.bottom > container.bottom) return item.bottom - container.bottom + padding;
  return 0;
}

export function parseSyncDate(value) {
  const match = /^(\d{4})[.-](\d{2})[.-](\d{2})$/.exec(value ?? "");
  if (!match) return null;
  const timestamp = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isNaN(timestamp) ? null : timestamp;
}

export function timelineProgress(start, end, reached) {
  if (start === null || reached === null || end <= start) return null;
  const percentage = ((reached - start) / (end - start)) * 100;
  return Math.max(1, Math.min(98, percentage));
}
