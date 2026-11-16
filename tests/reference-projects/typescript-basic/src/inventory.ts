export interface Item {
  sku: string;
  name: string;
  qty: number;
}

export function bySku<T extends { sku: string }>(items: T[]): Map<string, T> {
  return new Map(items.map((item) => [item.sku, item]));
}
