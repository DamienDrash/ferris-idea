package org.example.library.notify;

import jakarta.enterprise.context.ApplicationScoped;

@ApplicationScoped
public class StandardCourier implements Courier {
    @Override
    public void deliver(long bookId) {
    }
}
