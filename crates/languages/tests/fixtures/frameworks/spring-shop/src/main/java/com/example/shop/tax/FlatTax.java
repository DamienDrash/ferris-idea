package com.example.shop.tax;

public class FlatTax implements TaxCalculator {
    @Override
    public long tax(long cents) {
        return cents / 5;
    }
}
