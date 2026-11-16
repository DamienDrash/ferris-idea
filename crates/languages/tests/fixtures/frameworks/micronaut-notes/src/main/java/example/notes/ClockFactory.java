package example.notes;

import io.micronaut.context.annotation.Bean;
import io.micronaut.context.annotation.Factory;
import jakarta.inject.Singleton;
import java.time.Clock;

@Factory
public class ClockFactory {

    @Singleton
    public Clock clock() {
        return Clock.systemUTC();
    }

    @Bean
    public Stamp stamp(Clock clock) {
        return new Stamp();
    }

    public static class Stamp {
    }
}
