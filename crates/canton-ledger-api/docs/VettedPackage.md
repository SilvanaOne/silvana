# VettedPackage

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**package_id** | **String** | Package ID of this package. Always present. | 
**valid_from_inclusive** | Option<**String**> | The time from which this package is vetted. Empty if vetting time has no lower bound. | [optional]
**valid_until_exclusive** | Option<**String**> | The time until which this package is vetted. Empty if vetting time has no upper bound. | [optional]
**package_name** | **String** | Name of this package. Only available if the package has been uploaded to the current participant. If unavailable, is empty string. | 
**package_version** | **String** | Version of this package. Only available if the package has been uploaded to the current participant. If unavailable, is empty string. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


