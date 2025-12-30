# VettedPackages

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**packages** | Option<[**Vec<models::VettedPackage>**](VettedPackage.md)> | Sorted by package_name and package_version where known, and package_id as a last resort. | [optional]
**participant_id** | **String** | Participant on which these packages are vetted. Always present. | 
**synchronizer_id** | **String** | Synchronizer on which these packages are vetted. Always present. | 
**topology_serial** | **i32** | Serial of last ``VettedPackages`` topology transaction of this participant and on this synchronizer. Always present. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


