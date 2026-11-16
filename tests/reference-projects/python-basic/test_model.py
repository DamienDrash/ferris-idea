from src.inventory.model import Item, restock


def test_total():
    assert Item("A-1", "Bolt", 3, 25).total_cents() == 75


def test_restock():
    items = [Item("A-1", "Bolt", 3, 25)]
    assert restock(items, "A-1", 2).qty == 5
