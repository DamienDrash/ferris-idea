import { bySku, Item } from "./inventory";

const items: Item[] = [{ sku: "A-1", name: "Bolt", qty: 3 }];
console.log(bySku(items).get("A-1")?.qty);
