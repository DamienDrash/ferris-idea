package com.example.shop.payment;

import org.springframework.stereotype.Component;

@Component
public class StripeGateway implements PaymentGateway {
    @Override
    public void charge(long cents) {
    }
}
