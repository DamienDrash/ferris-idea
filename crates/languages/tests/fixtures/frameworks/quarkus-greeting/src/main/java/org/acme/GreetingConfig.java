package org.acme;

import io.smallrye.config.ConfigMapping;
import io.smallrye.config.WithDefault;

@ConfigMapping(prefix = "greeting")
public interface GreetingConfig {
    String message();

    @WithDefault("!")
    String suffix();
}
