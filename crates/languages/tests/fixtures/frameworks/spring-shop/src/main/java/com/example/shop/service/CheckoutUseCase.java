package com.example.shop.service;

import com.example.shop.config.ShopProperties;
import com.example.shop.model.Order;
import com.example.shop.notify.Notifier;
import com.example.shop.repository.OrderRepository;
import java.time.Clock;

@UseCase
public class CheckoutUseCase {
    private final OrderRepository orders;
    private final ShopProperties properties;
    private final Clock clock;
    private final Notifier notifier;

    public CheckoutUseCase(OrderRepository orders, ShopProperties properties, Clock clock, Notifier notifier) {
        this.orders = orders;
        this.properties = properties;
        this.clock = clock;
        this.notifier = notifier;
    }

    public Order place(Order order) {
        return orders.save(order);
    }
}
