# InstrumentConfigurationPayload

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**operator** | **String** | The operator party id | 
**provider** | **String** | The provider party id | 
**registrar** | **String** | The registrar party id | 
**default_identifier** | [**models::InstrumentIdentifier**](InstrumentIdentifier.md) |  | 
**additional_identifiers** | [**Vec<models::InstrumentIdentifier>**](InstrumentIdentifier.md) | Additional instrument identifiers | 
**issuer_requirements** | [**Vec<models::PartyCredentialRequirement>**](PartyCredentialRequirement.md) | Credential requirements to mint/burn a given asset | 
**holder_requirements** | [**Vec<models::PartyCredentialRequirement>**](PartyCredentialRequirement.md) | Credential requirements to transfer/lock/unlock a given asset | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


