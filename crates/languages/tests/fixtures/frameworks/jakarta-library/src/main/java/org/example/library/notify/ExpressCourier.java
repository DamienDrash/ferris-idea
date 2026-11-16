package org.example.library.notify;

import jakarta.enterprise.context.ApplicationScoped;

@Express
@ApplicationScoped
public class ExpressCourier implements Courier {
    @Override
    public void deliver(long bookId) {
    }
}
