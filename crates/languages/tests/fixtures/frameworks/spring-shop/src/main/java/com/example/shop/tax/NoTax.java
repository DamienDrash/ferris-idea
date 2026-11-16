package com.example.shop.tax;

public class NoTax implements TaxCalculator {
    @Override
    public long tax(long cents) {
        return 0;
    }
}
