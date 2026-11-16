package org.example.library.rest;

import jakarta.ws.rs.GET;
import jakarta.ws.rs.HEAD;
import jakarta.ws.rs.Path;
import jakarta.ws.rs.Produces;

@Path("health")
public class HealthResource {

    @GET
    @Produces("text/plain")
    public String health() {
        return "UP";
    }

    @HEAD
    @Path("live")
    public void live() {
    }
}
