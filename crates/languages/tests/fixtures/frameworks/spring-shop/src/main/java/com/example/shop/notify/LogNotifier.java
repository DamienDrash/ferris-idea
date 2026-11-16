package com.example.shop.notify;

import org.springframework.stereotype.Component;

@Component
public class LogNotifier implements Notifier {
    @Override
    public void send(String text) {
    }
}
