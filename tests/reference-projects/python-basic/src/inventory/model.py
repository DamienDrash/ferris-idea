from dataclasses import dataclass


@dataclass
class Item:
    sku: str
    name: str
    qty: int
    unit_price_cents: int

    def total_cents(self) -> int:
        return self.qty * self.unit_price_cents


def restock(items: list[Item], sku: str, amount: int) -> Item:
    for item in items:
        if item.sku == sku:
            item.qty += amount
            return item
    raise KeyError(sku)
