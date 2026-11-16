package org.acme;

import jakarta.enterprise.context.ApplicationScoped;

@ApplicationScoped
public class GreetingService {

    private final Formatter upper;

    // Quarkus: a single constructor needs no @Inject.
    public GreetingService(UpperFormatter upper) {
        this.upper = upper;
    }

    public String greet(String name) {
        return upper.format("hello " + name);
    }
}
