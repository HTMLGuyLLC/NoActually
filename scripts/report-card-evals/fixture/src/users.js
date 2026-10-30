const USERS = [
  { id: 0, first: 'Ada', last: 'Lovelace', created: '2024-01-03' },
  { id: 1, first: 'Linus', last: 'Pauling', created: '2024-02-11' },
  { id: 2, first: 'Grace', last: 'Hopper', created: '2023-11-20' },
];

export function getUser(id) {
  if (!id) return undefined;
  return USERS.find((u) => u.id === id);
}

export function listUsers() {
  return [...USERS];
}
