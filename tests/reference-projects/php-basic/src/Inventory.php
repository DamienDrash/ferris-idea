<?php
declare(strict_types=1);

namespace Lab\Inventory;

final class Item
{
    public function __construct(
        public readonly string $sku,
        public readonly string $name,
        public int $qty,
        public readonly int $unitPriceCents,
    ) {}

    public function totalCents(): int
    {
        return $this->qty * $this->unitPriceCents;
    }
}

function totalValue(Item ...$items): int
{
    return array_sum(array_map(fn (Item $item) => $item->totalCents(), $items));
}
