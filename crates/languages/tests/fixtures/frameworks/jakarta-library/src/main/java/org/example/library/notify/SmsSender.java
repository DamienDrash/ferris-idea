package org.example.library.notify;

import jakarta.ejb.Stateless;
import jakarta.inject.Named;

@Named("sms")
@Stateless
public class SmsSender implements Sender {
    @Override
    public void send(String text) {
    }
}
