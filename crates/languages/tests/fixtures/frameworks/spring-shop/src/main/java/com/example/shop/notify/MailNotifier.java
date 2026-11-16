package com.example.shop.notify;

import org.springframework.context.annotation.Primary;
import org.springframework.stereotype.Component;

@Primary
@Component("mail")
public class MailNotifier implements Notifier {
    @Override
    public void send(String text) {
    }
}
