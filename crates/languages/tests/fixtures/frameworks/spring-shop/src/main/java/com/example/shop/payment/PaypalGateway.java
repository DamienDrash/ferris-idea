package com.example.shop.payment;

import org.springframework.stereotype.Component;

@Component
public class PaypalGateway implements PaymentGateway {
    @Override
    public void charge(long cents) {
    }
}
