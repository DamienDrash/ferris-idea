package org.acme;

import jakarta.inject.Inject;
import jakarta.ws.rs.GET;
import jakarta.ws.rs.POST;
import jakarta.ws.rs.Path;
import jakarta.ws.rs.PathParam;
import jakarta.ws.rs.Produces;
import jakarta.ws.rs.core.MediaType;
import java.util.Optional;
import org.eclipse.microprofile.config.inject.ConfigProperty;
import org.eclipse.microprofile.rest.client.inject.RestClient;

@Path("/hello")
public class GreetingResource {

    // Defined in application.properties.
    @ConfigProperty(name = "greeting.message")
    String message;

    // Not defined, with a default.
    @ConfigProperty(name = "greeting.suffix", defaultValue = "!")
    String suffix;

    // Not defined, optional: no error at start.
    @ConfigProperty(name = "greeting.name")
    Optional<String> name;

    // Defined only for the dev profile (%dev.).
    @ConfigProperty(name = "greeting.debug")
    boolean debug;

    // Defined nowhere.
    @ConfigProperty(name = "greeting.missing")
    String missing;

    @Inject
    GreetingService service;

    // A config mapping interface: Quarkus provides the bean.
    @Inject
    GreetingConfig config;

    // A REST client interface: Quarkus provides the bean.
    @Inject
    @RestClient
    CountryClient countries;

    // Ambiguous: two formatters.
    @Inject
    Formatter formatter;

    // Missing: no scope annotation, no producer.
    @Inject
    Translator translator;

    @GET
    @Produces(MediaType.TEXT_PLAIN)
    public String hello() {
        return message + suffix;
    }

    @GET
    @Path("/{name}")
    @Produces(MediaType.TEXT_PLAIN)
    public String greet(@PathParam("name") String name) {
        return service.greet(name);
    }

    @POST
    @Path("/farewell")
    @Produces(MediaType.APPLICATION_JSON)
    public Farewell farewell(Farewell farewell) {
        return farewell;
    }
}
