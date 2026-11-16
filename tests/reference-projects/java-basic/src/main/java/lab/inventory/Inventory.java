package lab.inventory;

import java.util.List;

public final class Inventory {
    public record Item(String sku, String name, int qty, int unitPriceCents) {
        int totalCents() {
            return qty * unitPriceCents;
        }
    }

    public static int totalValue(List<Item> items) {
        return items.stream().mapToInt(Item::totalCents).sum();
    }

    public static void main(String[] args) {
        System.out.println(totalValue(List.of(new Item("A-1", "Bolt", 3, 25))));
    }
}
