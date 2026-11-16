package org.acme;

import jakarta.inject.Singleton;

@Singleton
public class LowerFormatter implements Formatter {
    @Override
    public String format(String text) {
        return text.toLowerCase();
    }
}
