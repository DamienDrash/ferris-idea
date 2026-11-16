package com.example.shop.payment;

public interface PaymentGateway {
    void charge(long cents);
}
