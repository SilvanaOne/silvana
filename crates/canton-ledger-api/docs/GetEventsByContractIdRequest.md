# GetEventsByContractIdRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**contract_id** | **String** | The contract id being queried. Required | 
**requesting_parties** | Option<**Vec<String>**> | Provided for backwards compatibility, it will be removed in the Canton version 3.4.0. The parties whose events the client expects to see. The events associated with the contract id will only be returned if the requesting parties includes at least one party that is a stakeholder of the event. For a definition of stakeholders see https://docs.daml.com/concepts/ledger-model/ledger-privacy.html#contract-observers-and-stakeholders Optional, if some parties specified, event_format needs to be unset. | [optional]
**event_format** | Option<[**models::EventFormat**](EventFormat.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


