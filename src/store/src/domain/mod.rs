use wot_accounts::contract::{PurchasableOffer, PurchasableOfferRef, StoreOrderError, StorePrice};

pub(crate) struct StoreOrder {
    pub item: i32,
    pub revision: i64,
    pub price: StorePrice,
}

#[tracing::instrument(name = "store.resolve_quote", skip_all, fields(
    ddd.bounded_context.name = "store", input.item = order.item,
    input.revision = order.revision, input.price.currency = order.price.currency(),
    input.price.amount = order.price.amount(),
    output.outcome = tracing::field::Empty
))]
pub(crate) fn resolve_quote<'a>(
    offers: impl Iterator<Item = PurchasableOfferRef<'a>>,
    order: StoreOrder,
) -> Result<PurchasableOffer, StoreOrderError> {
    let mut matching_item = offers.filter(|offer| offer.item() == order.item);
    let result = match matching_item.next() {
        None => Err(StoreOrderError::Unavailable),
        Some(first) => std::iter::once(first)
            .chain(matching_item)
            .find(|offer| offer.price().currency() == order.price.currency())
            .filter(|offer| offer.price() == order.price)
            .map(|offer| offer.to_owned())
            .ok_or(StoreOrderError::Stale),
    };
    tracing::Span::current().record(
        "output.outcome",
        match &result {
            Ok(_) => "quoted",
            Err(StoreOrderError::Unavailable) => "offer_unavailable",
            Err(_) => "stale",
        },
    );
    result
}
