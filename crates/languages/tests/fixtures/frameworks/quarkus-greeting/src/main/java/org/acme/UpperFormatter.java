package org.acme;

import jakarta.enterprise.context.ApplicationScoped;

@ApplicationScoped
public class UpperFormatter implements Formatter {
    @Override
    public String format(String text) {
        return text.toUpperCase();
    }
}
