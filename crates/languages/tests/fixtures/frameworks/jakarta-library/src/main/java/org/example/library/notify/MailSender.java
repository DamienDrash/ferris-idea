package org.example.library.notify;

import jakarta.enterprise.context.ApplicationScoped;
import jakarta.inject.Named;

@Named("mail")
@ApplicationScoped
public class MailSender implements Sender {
    @Override
    public void send(String text) {
    }
}
