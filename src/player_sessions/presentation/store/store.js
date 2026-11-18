"use strict";

const token = /^#([A-Za-z0-9_-]{43})(?=$|[/?&])/.exec(location.hash);
const offers = document.querySelector("#offers");
const balance = document.querySelector("#balance");
const status = document.querySelector("#status");
const refresh = document.querySelector("#refresh");
const number = new Intl.NumberFormat("en");
let catalog;

async function request(path, order) {
  const headers = { Authorization: "Bearer " + token[1] };
  const options = { headers, cache: "no-store", redirect: "error" };
  if (order) {
    headers["Content-Type"] = "application/json";
    options.method = "POST";
    options.body = JSON.stringify(order);
  }
  const response = await fetch(path, options);
  if (!response.ok) {
    throw new Error(await response.text());
  }
  return response.json();
}

function render() {
  balance.textContent = number.format(catalog.credits) + " credits available";
  balance.hidden = false;
  offers.textContent = "";
  for (const offer of catalog.offers) {
    const card = document.createElement("li");
    const title = document.createElement("h2");
    title.textContent = offer.name;
    const stock = document.createElement("p");
    stock.textContent = number.format(offer.in_depot) + " in Depot";
    const buy = document.createElement("button");
    buy.type = "button";
    buy.textContent = "Buy for " + number.format(offer.credits) + " credits";
    buy.disabled = catalog.credits < offer.credits;
    buy.addEventListener("click", () => purchase(offer));
    card.append(title, stock, buy);
    offers.append(card);
  }
}

async function load() {
  refresh.hidden = true;
  status.textContent = "Loading offers…";
  try {
    catalog = await request("/store/catalog");
    render();
    status.textContent = catalog.offers.length ? "" : "No equipment offers are available.";
  } catch (error) {
    status.textContent = error.message;
    refresh.hidden = false;
  }
}

async function purchase(offer) {
  for (const button of offers.querySelectorAll("button")) {
    button.disabled = true;
  }
  status.textContent = "Purchasing " + offer.name + "…";
  try {
    catalog = await request("/store/orders", {
      item: offer.item,
      revision: catalog.revision,
      credits: offer.credits
    });
    render();
    status.textContent = offer.name + " added to your Depot.";
  } catch (error) {
    status.textContent = error.message;
    refresh.hidden = false;
  }
}

refresh.addEventListener("click", load);
if (token) {
  load();
} else {
  status.textContent = "Open the Store from a connected garage.";
}
